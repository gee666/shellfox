use super::{
    darwin_io::{PtyIo, Stop},
    darwin_process::{exit_queue, Leader},
};
use crate::{config::size, input::InputWriter, reader::OutputReader};
use crate::{
    ExitStatus, NativeLaunchBinding, NativeLaunchEvidence, PtyConfig, PtyOutput, SpawnError,
};
use anyhow::{ensure, Context, Result};
use portable_pty::MasterPty;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{
    mpsc::{self, SyncSender, TryRecvError},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

pub struct PtySession {
    inner: Option<State>,
    reaper: SyncSender<State>,
}

struct State {
    master: Box<dyn MasterPty + Send>,
    leader: Leader,
    writer: InputWriter,
    output: OutputReader,
    io: Arc<Stop>,
    evidence: Option<NativeLaunchEvidence>,
    stopping: bool,
}

impl std::fmt::Debug for PtySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySession")
            .field("closed", &self.inner.is_none())
            .finish()
    }
}

impl PtySession {
    /// Read-only session-member evidence for stats; None once stopping.
    pub(crate) fn darwin_inspection(&self) -> Option<super::darwin_process::Inspection> {
        self.inner.as_ref().filter(|state| !state.stopping)?.leader.inspection()
    }

    pub fn spawn(config: PtyConfig) -> std::result::Result<Self, SpawnError> {
        Self::spawn_inner(config, None)
    }

    pub fn spawn_bound(
        config: PtyConfig,
        binding: NativeLaunchBinding,
    ) -> std::result::Result<Self, SpawnError> {
        Self::spawn_inner(config, Some(binding))
    }

    fn spawn_inner(
        config: PtyConfig,
        binding: Option<NativeLaunchBinding>,
    ) -> std::result::Result<Self, SpawnError> {
        config.validate()?;
        let uid = unsafe { libc::getuid() };
        ensure_normal_account(&config, uid)?;
        let command = match &config.command {
            Some(command) => command.clone(),
            None => match config.profile {
                Some(id) => crate::shell_profile(id)?,
                None => crate::default_shell_profile()?,
            }
            .command()?,
        };
        let program = checked_executable(Path::new(&command.program))?;
        let argv = std::iter::once(program.into_os_string())
            .chain(command.args)
            .collect::<Vec<_>>();
        let builder = config.builder(&argv)?;
        let queue = exit_queue().context("create macOS PTY exit queue")?;
        let (reaper, receiver) = mpsc::sync_channel::<State>(1);
        std::thread::Builder::new()
            .name("macos-pty-cleanup".into())
            .spawn(move || {
                while let Ok(mut state) = receiver.recv() {
                    while state.shutdown().is_err() {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .context("start retained macOS PTY owner")?;
        let pair = portable_pty::native_pty_system().openpty(size(config.cols, config.rows)?)?;
        let io = Arc::new(Stop::default());
        let (reader, writer) = PtyIo::pair(
            pair.master.as_raw_fd().context("missing PTY master fd")?,
            Arc::clone(&io),
        )
        .context("open cancellable macOS PTY I/O")?;
        let writer = InputWriter::start(Box::new(writer)).context("start PTY writer")?;
        let output = OutputReader::start(Box::new(reader)).context("start PTY reader")?;
        let child = pair.slave.spawn_command(builder);
        drop(pair.slave);
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                io.cancel.store(true, Ordering::Release);
                return Err(error.into());
            }
        };
        let mut state = State {
            master: pair.master,
            leader: Leader::new(child, queue),
            writer,
            output,
            io,
            evidence: None,
            stopping: false,
        };
        match state.leader.verify() {
            Ok(uid) => {
                state.evidence =
                    binding.map(|binding| crate::launch_privilege::capture_darwin(binding, uid));
                Ok(Self {
                    inner: Some(state),
                    reaper,
                })
            }
            Err(error) => {
                let mut session = Self {
                    inner: Some(state),
                    reaper,
                };
                match session.shutdown() {
                    Ok(()) => Err(error.into()),
                    Err(cleanup) => Err(SpawnError {
                        error: error.context(format!("cleanup unconfirmed: {cleanup:#}")),
                        session: Mutex::new(Some(Box::new(session))),
                    }),
                }
            }
        }
    }

    pub fn take_launch_evidence(&mut self) -> Option<NativeLaunchEvidence> {
        self.inner.as_mut().and_then(|state| state.evidence.take())
    }

    /// Registers the owner wake for queued output and root-process exit. Call once.
    pub fn set_wake(&self, wake: crate::Wake) -> Result<crate::ExitSignal> {
        let state = self.inner.as_ref().context("PTY closed")?;
        state.output.set_wake(wake.clone());
        crate::wake::watch_exit(state.leader.pid(), wake).context("start PTY exit watch")
    }

    pub fn try_read(&self) -> std::result::Result<Option<PtyOutput>, TryRecvError> {
        self.inner
            .as_ref()
            .ok_or(TryRecvError::Disconnected)?
            .output
            .try_read()
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let state = self.inner.as_mut().context("PTY closed")?;
        ensure!(!state.stopping, "PTY input disabled");
        state.writer.write(bytes)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let state = self.inner.as_ref().context("PTY closed")?;
        ensure!(!state.stopping, "PTY resize disabled");
        state.master.resize(size(cols, rows)?)
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        let Some(state) = self.inner.as_mut() else {
            return Ok(None);
        };
        let status = state.leader.poll()?;
        if status.is_some() {
            state.stopping = true;
            state.writer.stop();
            state.io.drain.store(true, Ordering::Release);
        }
        Ok(status)
    }

    pub fn begin_shutdown(&mut self) -> Result<()> {
        if let Some(state) = &mut self.inner {
            state.begin()?;
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<()> {
        if let Some(state) = &mut self.inner {
            state.shutdown()?;
        }
        self.inner = None;
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if let Some(mut state) = self.inner.take() {
            let _ = state.begin();
            if let Err(mut error) = self.reaper.send(state) {
                let _ = error.0.shutdown();
            }
        }
    }
}

impl State {
    fn begin(&mut self) -> Result<()> {
        self.stopping = true;
        self.writer.stop();
        self.leader.begin()
    }

    fn shutdown(&mut self) -> Result<()> {
        self.output.cancel();
        self.io.cancel.store(true, Ordering::Release);
        self.begin()?;
        let deadline = Instant::now() + Duration::from_secs(7);
        while self.leader.poll()?.is_none() || !self.writer.finished() || !self.output.finished() {
            ensure!(Instant::now() < deadline, "macOS PTY cleanup not confirmed");
            std::thread::sleep(Duration::from_millis(5));
        }
        self.writer.join()?;
        self.output.join()
    }
}

fn checked_executable(program: &Path) -> Result<std::path::PathBuf> {
    let program = program.canonicalize()?;
    let metadata = program.metadata()?;
    let mode = metadata.permissions().mode();
    ensure!(
        metadata.is_file() && mode & 0o111 != 0 && mode & 0o6000 == 0,
        "macOS PTY requires an executable without setuid/setgid bits"
    );
    Ok(program)
}

fn ensure_normal_account(config: &PtyConfig, uid: libc::uid_t) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == uid && unsafe { libc::getegid() == libc::getgid() },
        "macOS PTY requires unchanged real and effective credentials"
    );
    ensure!(
        config.unix_privilege != crate::UnixPrivilegeMode::NonElevating || uid != 0,
        "remote-eligible macOS PTY requires a non-root launch account"
    );
    Ok(())
}
