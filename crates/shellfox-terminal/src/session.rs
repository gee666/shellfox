use crate::config::size;
use crate::input::InputWriter;
use crate::reader::OutputReader;
use crate::runtime::{start_reaper, ConsoleCloser, Session};
use crate::{ExitStatus, PtyConfig, PtyOutput, SpawnError};
use anyhow::{ensure, Context, Result};
use std::sync::mpsc::{self, SyncSender, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
#[cfg(all(test, feature = "test-support"))]
#[path = "session_launch_tests.rs"]
mod launch_tests;

#[derive(Default)]
pub(crate) struct SpawnOptions {
    pub deny_assignment: bool,
    pub skip_release: bool,
    pub fail_ready_cleanup: bool,
}

pub struct PtySession {
    pub(crate) inner: Option<Session>,
    // Prestarted before child creation. Drop can transfer failed cleanup without
    // abandoning ownership or trying to start a thread during error recovery.
    reaper: SyncSender<Session>,
}
impl std::fmt::Debug for PtySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySession")
            .field("closed", &self.inner.is_none())
            .finish()
    }
}

impl PtySession {
    pub fn spawn_bound(
        config: PtyConfig,
        binding: crate::NativeLaunchBinding,
    ) -> std::result::Result<Self, SpawnError> {
        Self::spawn_windows_inner(config, SpawnOptions::default(), Some(binding))
    }
    pub fn take_launch_evidence(&mut self) -> Option<crate::NativeLaunchEvidence> {
        self.inner
            .as_mut()
            .and_then(|inner| inner.launch_evidence.take())
    }
    pub fn spawn(config: PtyConfig) -> std::result::Result<Self, SpawnError> {
        Self::spawn_windows(config, SpawnOptions::default())
    }

    #[cfg(windows)]
    pub(crate) fn spawn_windows(
        config: PtyConfig,
        options: SpawnOptions,
    ) -> std::result::Result<Self, SpawnError> {
        Self::spawn_windows_inner(config, options, None)
    }
    fn spawn_windows_inner(
        config: PtyConfig,
        options: SpawnOptions,
        binding: Option<crate::NativeLaunchBinding>,
    ) -> std::result::Result<Self, SpawnError> {
        use crate::windows::{Job, StartupGate, STARTUP_TIMEOUT_MS, SUPERVISOR_FLAG};
        config.validate()?;
        let mut launch_binding = binding;
        let job = Job::new()?;
        let gate = StartupGate::new()?;
        let mut inspection_gate = crate::inspection_gate::InspectionGate::new(&gate.release_name)?;
        let command = match &config.command {
            Some(command) => command.clone(),
            None => match config.profile {
                Some(id) => crate::shell_profile(id)?,
                None => crate::default_shell_profile()?,
            }
            .command()?,
        };
        let supervisor = config
            .supervisor
            .as_ref()
            .context("trusted supervisor missing")?;
        let mut argv = vec![
            supervisor.as_os_str().to_owned(),
            SUPERVISOR_FLAG.into(),
            gate.release_name.clone().into(),
            gate.ready_name.clone().into(),
        ];
        argv.extend(inspection_gate.arguments());
        argv.push(command.program);
        argv.extend(command.args);
        let builder = config.builder(&argv)?;
        let pair = portable_pty::native_pty_system().openpty(size(config.cols, config.rows)?)?;
        let writer =
            InputWriter::start(pair.master.take_writer()?).context("start bounded PTY writer")?;
        let output = OutputReader::start(pair.master.try_clone_reader()?)
            .context("start bounded PTY reader")?;
        let closer = ConsoleCloser::start().context("start PTY close worker")?;
        let reaper = start_reaper().context("start PTY cleanup owner")?;
        // No fallible thread/pipe setup after this point can orphan a child.
        let child = pair.slave.spawn_command(builder)?;
        drop(pair.slave);
        let mut session = Self {
            inner: Some(Session {
                child,
                inspection: None,
                launch_evidence: None,
                master: Some(pair.master),
                writer,
                output,
                closer,
                status: None,
                ready: false,
                released: false,
                job: Some(job),
                cleanup_started: false,
                #[cfg(feature = "test-support")]
                inject_cleanup_failure: options.fail_ready_cleanup,
            }),
            reaper,
        };
        let startup = (|| -> Result<()> {
            let inner = session.inner.as_mut().context("missing session")?;
            ensure!(!options.deny_assignment, "injected assignment denial");
            inner
                .job
                .as_ref()
                .context("missing Job")?
                .assign(inner.child.as_ref())?;
            // From now on cleanup always terminates the whole Job.
            inner.released = true;
            if !options.skip_release {
                gate.release()?;
            }
            let deadline = Instant::now() + Duration::from_millis(STARTUP_TIMEOUT_MS as u64);
            loop {
                if inner.inspection.is_none() {
                    inner.inspection = inspection_gate.try_capture(
                        inner.job.as_ref().context("missing Job")?,
                        inner.child.as_ref(),
                    )?;
                }
                if gate.ready()? {
                    ensure!(
                        inner.inspection.is_some(),
                        "logical child proof missing before READY"
                    );
                    ensure!(!options.fail_ready_cleanup, "injected post-READY failure");
                    if let Some(binding) = launch_binding.take() {
                        inner.launch_evidence = Some(
                            inner
                                .inspection
                                .as_ref()
                                .context("missing pinned child")?
                                .launch_evidence(binding),
                        );
                    }
                    inner.ready = true;
                    return Ok(());
                }
                ensure!(
                    inner.poll_root()?.is_none(),
                    "supervisor exited before READY"
                );
                ensure!(Instant::now() < deadline, "supervisor READY timed out");
                thread::sleep(Duration::from_millis(2));
            }
        })();
        if let Err(error) = startup {
            let mut diagnostic = Vec::new();
            while diagnostic.len() < 8192 {
                match session.try_read() {
                    Ok(Some(PtyOutput::Data(bytes))) => diagnostic.extend(bytes),
                    _ => break,
                }
            }
            let error = error.context(format!(
                "startup PTY output: {:?}",
                String::from_utf8_lossy(&diagnostic)
            ));
            return match session.shutdown() {
                Ok(()) => Err(error.into()),
                Err(cleanup) => Err(SpawnError {
                    error: error.context(format!("cleanup unconfirmed: {cleanup:#}")),
                    session: Mutex::new(Some(Box::new(session))),
                }),
            };
        }
        Ok(session)
    }

    /// Registers the owner wake for queued output and root-process exit. Call once.
    pub fn set_wake(&self, wake: crate::Wake) -> Result<crate::ExitSignal> {
        let inner = self.inner.as_ref().context("PTY is closed")?;
        inner.output.set_wake(wake.clone());
        let process = inner.child.as_raw_handle().context("PTY child handle missing")?;
        crate::wake::watch_exit(process, wake).context("start PTY exit watch")
    }

    pub fn try_read(&self) -> std::result::Result<Option<PtyOutput>, TryRecvError> {
        self.inner
            .as_ref()
            .ok_or(TryRecvError::Disconnected)?
            .output
            .try_read()
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let inner = self.inner.as_mut().context("PTY is closed")?;
        ensure!(
            inner.ready && !inner.cleanup_started,
            "PTY is not ready for input"
        );
        let result = inner.writer.write(bytes);
        if result.is_err() {
            inner.ready = false;
        }
        result
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let requested = size(cols, rows)?;
        let inner = self.inner.as_ref().context("PTY is closed")?;
        ensure!(
            inner.ready && !inner.cleanup_started,
            "PTY is not ready for resize"
        );
        inner
            .master
            .as_ref()
            .context("PTY console closed")?
            .resize(requested)
    }

    /// Polling also contains descendants after the root exits. EOF follows final
    /// output, not merely root exit. Keep polling while the window remains open.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(None);
        };
        let status = inner.poll_root()?;
        if status.is_some() {
            inner.begin_cleanup()?;
            if inner.processes_gone()? {
                inner.close_console()?;
            }
        }
        Ok(status)
    }

    /// Stop owned processes and input without cancelling the output reader.
    /// Continue polling and draining through EOF, then call `shutdown` to join
    /// workers. Any failure retains all owners and leaves input disabled.
    pub fn begin_shutdown(&mut self) -> Result<()> {
        if let Some(inner) = self.inner.as_mut() {
            inner.begin_cleanup()?;
            if inner.processes_gone()? {
                inner.close_console()?;
            }
        }
        Ok(())
    }

    /// On failure every remaining owner stays in this session for retry.
    /// Explicit shutdown cancels queued output; drain to EOF first to preserve it.
    pub fn shutdown(&mut self) -> Result<()> {
        if let Some(inner) = self.inner.as_mut() {
            inner.shutdown()?;
        }
        self.inner = None;
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if let Some(mut inner) = self.inner.take() {
            if inner.shutdown().is_err() {
                if let Err(mpsc::SendError(retained)) = self.reaper.send(inner) {
                    // Last-resort retention if the prestarted cleanup owner panics.
                    // Never drop an unconfirmed owned process/controller silently.
                    std::mem::forget(retained);
                }
            }
        }
    }
}
