use super::input::Input;
use super::*;
use crate::{
    raw_process::RawStream,
    UnixPrivilegeMode,
    unix::{cgroup::Cgroup, startup::Gate},
};
use anyhow::ensure;
use std::{
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::Duration,
};
impl RawProcess {
    pub fn spawn(
        supervisor: &Path,
        command: RawCommand,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn_inner(supervisor, command, None, false, false, false)
    }
    pub fn spawn_with_launch_binding(
        supervisor: &Path,
        command: RawCommand,
        binding: NativeLaunchBinding,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn_inner(supervisor, command, Some(binding), false, false, false)
    }
    fn spawn_inner(
        supervisor: &Path,
        command: RawCommand,
        binding: Option<NativeLaunchBinding>,
        deny_assignment: bool,
        skip_release: bool,
        fail_cleanup: bool,
    ) -> std::result::Result<Self, RawSpawnError> {
        ensure_waitable_host()?;
        let supervisor = crate::unix::private::helper_path(supervisor)?;
        let command = command.resolved()?;
        let specification = serde_json::to_vec(&command).map_err(anyhow::Error::from)?;
        let group = Cgroup::create()?;
        let gate = Gate::new()?;
        let runner = crate::unix::private::helper_path(Path::new("/usr/bin/systemd-run"))?;
        // Match the ordinary shareable PTY: the pinned supervisor and the
        // target it execs must not gain privileges at exec time.
        let uid = unsafe { libc::getuid() };
        let mode = if binding.is_some() && uid != 0 && uid == unsafe { libc::geteuid() } {
            UnixPrivilegeMode::NonElevating
        } else {
            UnixPrivilegeMode::Local
        };
        let arguments = crate::unix::linux::scope::scope_arguments(
            &crate::unix::linux::scope::fresh_nonce()?,
            &supervisor,
            &gate.raw_arguments(mode),
        )?;
        let (reaper, incoming) = mpsc::sync_channel::<Inner>(1);
        std::thread::Builder::new()
            .name("linux-raw-cleanup".into())
            .spawn(move || {
                while let Ok(mut inner) = incoming.recv() {
                    while inner.close().is_err() {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .map_err(anyhow::Error::from)?;
        // Target cwd/argv/env travel ONLY on the authenticated startup socket.
        // No frame bytes are put in target stdin or the controller environment.
        let mut builder = Command::new(runner);
        builder
            .arg("--expand-environment=no")
            .args(arguments)
            .current_dir("/")
            .env_clear()
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path=/run/user/{}/bus", unsafe { libc::getuid() }),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            builder.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = builder
            .spawn()
            .map_err(|_| anyhow::anyhow!("Trusted raw controller launch failed"))?;
        let mut process = Self {
            pid: child.id(),
            inner: Some(Inner::new(child, group)),
            reaper,
            input_ended: false,
            ready: false,
            stopped: false,
            closed_status: None,
            closed_signal9: false,
            launch_evidence: None,
        };
        let mut evidence = None;
        let startup = (|| -> Result<()> {
            let inner = process.inner.as_mut().context("Raw owner missing")?;
            inner.pin()?;
            inner.readers.push(inner.output.reader(
                inner.child.stdout.take().context("Raw stdout missing")?,
                RawStream::Stdout,
            )?);
            inner.readers.push(inner.output.reader(
                inner.child.stderr.take().context("Raw stderr missing")?,
                RawStream::Stderr,
            )?);
            inner.input = Some(Input::start(
                inner.child.stdin.take().context("Raw stdin missing")?,
            )?);
            inner.control = Some(gate.release_raw(inner.child.id(), &specification, || {
                ensure!(!deny_assignment, "Injected raw assignment denial before GO");
                inner.attach()?;
                ensure!(!skip_release, "Injected raw cancellation before GO");
                if let Some(binding) = binding {
                    evidence = Some(crate::launch_privilege::capture_raw_posix(
                        binding,
                        inner.child.id(),
                        inner.pidfd.as_ref().context("Original raw pidfd missing")?,
                        mode,
                    )?);
                }
                Ok(())
            })?);
            ensure!(!fail_cleanup, "Injected post-START cleanup uncertainty");
            Ok(())
        })();
        if let Err(error) = startup {
            if fail_cleanup || process.close().is_err() {
                return Err(RawSpawnError {
                    error: error.context("Raw cleanup unconfirmed; owner retained"),
                    retained: Mutex::new(Some(Box::new(process))),
                });
            }
            return Err(error.into());
        }
        process.launch_evidence = evidence;
        process.ready = true;
        Ok(process)
    }
    #[cfg(feature = "test-support")]
    pub fn spawn_fault(
        supervisor: &Path,
        command: RawCommand,
        deny_assignment: bool,
        skip_release: bool,
        fail_cleanup: bool,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn_inner(
            supervisor,
            command,
            None,
            deny_assignment,
            skip_release,
            fail_cleanup,
        )
    }
    #[cfg(feature = "test-support")]
    pub fn spawn_fault_with_launch_binding(
        supervisor: &Path,
        command: RawCommand,
        binding: NativeLaunchBinding,
        deny_assignment: bool,
        skip_release: bool,
        fail_cleanup: bool,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn_inner(
            supervisor,
            command,
            Some(binding),
            deny_assignment,
            skip_release,
            fail_cleanup,
        )
    }
}

fn ensure_waitable_host() -> Result<()> {
    let mut disposition = std::mem::MaybeUninit::<libc::sigaction>::uninit();
    ensure!(
        unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), disposition.as_mut_ptr()) } == 0,
        "Raw original-child wait capability unavailable"
    );
    let disposition = unsafe { disposition.assume_init() };
    ensure!(
        disposition.sa_sigaction != libc::SIG_IGN && disposition.sa_flags & libc::SA_NOCLDWAIT == 0,
        "Raw host must retain waitable original children"
    );
    Ok(())
}
