use super::input::Input;
use super::*;
use crate::raw_process::RawStream;
use anyhow::ensure;
use std::{process::Stdio, time::Duration};
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
        let command = command.resolved()?;
        let specification = serde_json::to_vec(&command).map_err(anyhow::Error::from)?;
        let owner::Launch {
            command: mut builder,
            owner,
            mut gate,
        } = owner::prepare(supervisor, &command, binding.is_some())?;
        let (reaper, incoming) = mpsc::sync_channel::<Inner>(1);
        std::thread::Builder::new()
            .name("raw-cleanup".into())
            .spawn(move || {
                while let Ok(mut inner) = incoming.recv() {
                    while inner.close().is_err() {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .map_err(anyhow::Error::from)?;
        builder
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = builder
            .spawn()
            .map_err(|_| anyhow::anyhow!("Trusted raw controller launch failed"))?;
        let mut process = Self {
            pid: child.id(),
            inner: Some(Inner::new(child, owner)),
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
            inner.owner.pin(&inner.child)?;
            let stdout = inner.child.stdout.take().context("Raw stdout missing")?;
            let stdout = inner.owner.reader(stdout)?;
            inner
                .readers
                .push(inner.output.reader(stdout, RawStream::Stdout)?);
            let stderr = inner.child.stderr.take().context("Raw stderr missing")?;
            let stderr = inner.owner.reader(stderr)?;
            inner
                .readers
                .push(inner.output.reader(stderr, RawStream::Stderr)?);
            inner.input = Some(Input::start(
                inner.child.stdin.take().context("Raw stdin missing")?,
            )?);
            inner.control = Some(gate.release(inner.child.id(), &specification, || {
                ensure!(!deny_assignment, "Injected raw assignment denial before GO");
                inner.attach()?;
                ensure!(!skip_release, "Injected raw cancellation before GO");
                if let Some(binding) = binding {
                    evidence = Some(inner.owner.evidence(binding, &inner.child)?);
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
