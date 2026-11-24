use super::{boundary::Boundary, startup::Gate, state::State};
use crate::{config::size, input::InputWriter, reader::OutputReader};
use crate::{ExitStatus, NativeLaunchBinding, PtyConfig, PtyOutput, SpawnError};
use anyhow::{ensure, Context, Result};
use std::sync::{
    mpsc::{self, SyncSender, TryRecvError},
    Mutex,
};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct SpawnOptions {
    #[cfg(feature = "test-support")]
    pub fault: Option<crate::StartupFault>,
}
impl SpawnOptions {
    fn before_assignment(&self) -> Result<()> {
        #[cfg(feature = "test-support")]
        ensure!(
            !matches!(self.fault, Some(crate::StartupFault::AssignmentDenied)),
            "injected assignment denial before GO"
        );
        Ok(())
    }
    fn before_go(&self) -> Result<()> {
        #[cfg(feature = "test-support")]
        ensure!(
            !matches!(self.fault, Some(crate::StartupFault::NoRelease)),
            "injected startup cancellation before GO"
        );
        Ok(())
    }
    fn after_ready(&self, state: &mut State) -> Result<()> {
        #[cfg(not(feature = "test-support"))]
        let _ = state;
        #[cfg(feature = "test-support")]
        if matches!(self.fault, Some(crate::StartupFault::ReadyCleanupFailure)) {
            state.inject_cleanup_failure = true;
            anyhow::bail!("injected post-READY failure");
        }
        Ok(())
    }
}
pub struct PtySession {
    inner: Option<State>,
    reaper: SyncSender<State>,
}
impl std::fmt::Debug for PtySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySession")
            .field("closed", &self.inner.is_none())
            .finish()
    }
}
impl PtySession {
    pub fn inspection_capability(
        &self,
    ) -> std::result::Result<
        crate::inspection::InspectionCapability,
        crate::inspection::InspectionFailure,
    > {
        use crate::inspection::{InspectionCapability, InspectionFailure::Unavailable};
        let state = self
            .inner
            .as_ref()
            .filter(|state| state.ready && !state.stopping)
            .ok_or(Unavailable)?;
        let pid = state
            .child
            .as_ref()
            .and_then(|child| child.process_id())
            .ok_or(Unavailable)?;
        let group = state
            .group
            .as_ref()
            .ok_or(Unavailable)?
            .inspection()
            .map_err(|_| Unavailable)?;
        let supervisor = state
            .pidfd
            .as_ref()
            .ok_or(Unavailable)?
            .try_clone()
            .map_err(|_| Unavailable)?;
        Ok(InspectionCapability(std::sync::Arc::new(
            super::linux::inspection::OwnedInspection::new(group, supervisor, pid),
        )))
    }

    pub fn spawn(config: PtyConfig) -> std::result::Result<Self, SpawnError> {
        Self::spawn_inner(config, None, SpawnOptions::default())
    }
    pub fn spawn_bound(
        config: PtyConfig,
        binding: NativeLaunchBinding,
    ) -> std::result::Result<Self, SpawnError> {
        Self::spawn_inner(config, Some(binding), SpawnOptions::default())
    }
    pub(crate) fn spawn_inner(
        config: PtyConfig,
        binding: Option<NativeLaunchBinding>,
        options: SpawnOptions,
    ) -> std::result::Result<Self, SpawnError> {
        config.validate()?;
        let command = match &config.command {
            Some(command) => command.clone(),
            None => match config.profile {
                Some(id) => crate::shell_profile(id)?,
                None => crate::default_shell_profile()?,
            }
            .command_for(&config.environment)?,
        };
        let supervisor = super::private::helper_path(
            config
                .supervisor
                .as_ref()
                .context("trusted supervisor missing")?,
        )?;
        let group = Boundary::create()?;
        let gate = Gate::new()?;
        let argv = group.command_line(
            &supervisor,
            &gate.arguments(&command, config.unix_privilege, group.containment()),
        )?;
        let mut builder = config.builder(&argv)?;
        if config.command.is_none()
            && std::path::Path::new(&command.program)
                .file_name()
                .is_some_and(|name| name == "bash")
        {
            builder.env(
                "PROMPT_COMMAND",
                prompt_command(builder.get_env("PROMPT_COMMAND")),
            );
        }
        // systemd-run needs the user bus. A user tab keeps it; the supervisor
        // drops it again for an isolated one.
        if group.containment() == super::Containment::Cgroup
            && builder.get_env("DBUS_SESSION_BUS_ADDRESS").is_none()
        {
            builder.env("DBUS_SESSION_BUS_ADDRESS", super::boundary::user_bus());
        }
        if config.environment != crate::TerminalEnvironment::User {
            builder.env(super::production_supervisor::ISOLATED, "1");
        }
        let pair = portable_pty::native_pty_system().openpty(size(config.cols, config.rows)?)?;
        let writer = InputWriter::start(pair.master.take_writer()?).context("start PTY input")?;
        let output =
            OutputReader::start(pair.master.try_clone_reader()?).context("start PTY output")?;
        let (reaper, receiver) = mpsc::sync_channel::<State>(1);
        std::thread::Builder::new()
            .name("linux-pty-cleanup".into())
            .spawn(move || {
                while let Ok(mut state) = receiver.recv() {
                    while state.shutdown().is_err() {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .context("start retained Linux PTY owner")?;
        let mut session = Self {
            inner: Some(State::new(group, pair.master, writer, output)),
            reaper,
        };
        let startup = (|| -> Result<()> {
            let state = session.inner.as_mut().context("PTY state missing")?;
            state.child = Some(pair.slave.spawn_command(builder)?);
            drop(pair.slave);
            let pid = state.pin()?;
            let bound = binding.is_some();
            let binding =
                binding.unwrap_or_else(|| crate::launch_privilege::new_launch_binding().0);
            state.control = Some(gate.release(pid, || {
                options.before_assignment()?;
                state.attach(pid)?;
                if let Err(error) = super::linux::scope::raise_child_oom_score(pid) {
                    eprintln!("Terminal OOM score unchanged: {error:#}");
                }
                options.before_go()?;
                let evidence = crate::launch_privilege::capture_posix(
                    binding,
                    pid,
                    state
                        .pidfd
                        .as_ref()
                        .context("original POSIX pidfd missing")?,
                    config.unix_privilege,
                )?;
                if bound {
                    state.evidence = Some(evidence);
                }
                Ok(())
            })?);
            state.ready = true;
            options.after_ready(state)?;
            Ok(())
        })();
        if let Err(error) = startup {
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
    #[cfg(feature = "test-support")]
    pub(crate) fn set_cleanup_failure(&mut self, fail: bool) {
        if let Some(state) = &mut self.inner {
            state.inject_cleanup_failure = fail;
        }
    }
    #[cfg(feature = "test-support")]
    pub fn disconnect_controller_for_test(&mut self) {
        if let Some(state) = &mut self.inner {
            state.control = None;
        }
    }
    #[cfg(feature = "test-support")]
    pub fn signal_controller_for_test(&self, signal: i32) -> Result<()> {
        use std::os::fd::AsRawFd;
        ensure!(
            matches!(signal, libc::SIGHUP | libc::SIGTERM | libc::SIGINT),
            "unsupported controller test signal"
        );
        let pidfd = self
            .inner
            .as_ref()
            .and_then(|state| state.pidfd.as_ref())
            .context("original controller pidfd missing")?;
        ensure!(
            unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    pidfd.as_raw_fd(),
                    signal,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            } == 0,
            "signal original controller: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
    pub fn take_launch_evidence(&mut self) -> Option<crate::NativeLaunchEvidence> {
        self.inner.as_mut().and_then(|s| s.evidence.take())
    }
    /// Registers the owner wake for queued output, root exit and tree teardown. Call once.
    pub fn set_wake(&self, wake: crate::Wake) -> Result<crate::ExitSignal> {
        let state = self.inner.as_ref().context("PTY closed")?;
        state.output.set_wake(wake.clone());
        let pid = state
            .child
            .as_ref()
            .and_then(|child| child.process_id())
            .context("PTY child missing")?;
        let exit = crate::wake::watch_exit(pid, wake.clone()).context("start PTY exit watch")?;
        state
            .group
            .as_ref()
            .context("PTY boundary missing")?
            .watch(wake)?;
        Ok(exit.with_teardown())
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
        ensure!(state.ready && !state.stopping, "PTY input disabled");
        let result = state.writer.write(bytes);
        if result.is_err() {
            state.ready = false;
        }
        result
    }
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let size = size(cols, rows)?;
        let state = self.inner.as_ref().context("PTY closed")?;
        ensure!(state.ready && !state.stopping, "PTY resize disabled");
        state
            .master
            .as_ref()
            .context("PTY master closed")?
            .resize(size)
    }
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        self.inner.as_mut().map_or(Ok(None), State::poll)
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
            if state.shutdown().is_err() {
                if let Err(mpsc::SendError(retained)) = self.reaper.send(state) {
                    std::mem::forget(retained);
                }
            }
        }
    }
}

/// The bash cwd hook runs first, then any PROMPT_COMMAND the tab inherited.
/// The hook is a subshell that exits with the previous status, so `$?` is
/// unchanged for the commands after it. An rc file that assigns
/// PROMPT_COMMAND without keeping the old value still replaces both.
fn prompt_command(inherited: Option<&std::ffi::OsStr>) -> std::ffi::OsString {
    let mut value = std::ffi::OsString::from(include_str!("../bash-prompt.sh").trim_end());
    if let Some(inherited) = inherited.filter(|value| !value.is_empty()) {
        value.push("\n");
        value.push(inherited);
    }
    value
}

#[cfg(test)]
#[test]
fn the_cwd_hook_keeps_an_inherited_prompt_command() {
    let hook = include_str!("../bash-prompt.sh").trim_end();
    assert_eq!(prompt_command(None), hook);
    assert_eq!(prompt_command(Some("".as_ref())), hook);
    assert_eq!(
        prompt_command(Some("history -a".as_ref())),
        format!("{hook}\nhistory -a").as_str()
    );
}
