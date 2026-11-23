#[path = "raw_process_cleanup.rs"]
mod cleanup;
#[path = "raw_process_launch.rs"]
mod launch;
use super::{
    io::{Input, Output},
    AutomationStdinClosed, AutomationStdinUnavailable, RawCommand, RawOutput, WriteReceipt,
};
use crate::windows::{Job, StartupGate, STARTUP_TIMEOUT_MS};
use anyhow::{ensure, Context, Result};
use std::{
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, SyncSender},
        Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::{
    JobObjects::{AssignProcessToJobObject, IsProcessInJob, TerminateJobObject},
    Threading::{TerminateProcess, CREATE_NO_WINDOW},
};

// A private forced-termination code distinguishes confirmed host termination
// from natural exits. It is internal kernel status, never an OS signal claim.
const FORCED_EXIT_CODE: u32 = 0x5346_5801;
pub struct RawProcess {
    inner: Option<Inner>,
    reaper: SyncSender<Inner>,
    pid: u32,
    input_ended: bool,
    closed_status: Option<i32>,
    stop_initiated: bool,
    launch_evidence: Option<crate::launch_privilege::NativeLaunchEvidence>,
}
struct Inner {
    child: Child,
    job: Job,
    assigned: bool,
    input: Option<Input>,
    output: Output,
    readers: Vec<JoinHandle<()>>,
    status: Option<i32>,
}
#[derive(Debug)]
pub struct RawSpawnError {
    error: anyhow::Error,
    retained: Mutex<Option<Box<RawProcess>>>,
}
impl RawSpawnError {
    pub fn take_process(&self) -> Option<RawProcess> {
        self.retained
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|p| *p)
    }
}
impl std::fmt::Debug for RawProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawProcess")
            .field("pid", &self.pid)
            .field("closed", &self.inner.is_none())
            .finish()
    }
}
impl std::fmt::Display for RawSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for RawSpawnError {}
impl From<anyhow::Error> for RawSpawnError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            error,
            retained: Mutex::new(None),
        }
    }
}
impl RawProcess {
    /// supervisor is a host-owned trusted executable, never a request field.
    pub fn spawn(
        supervisor: &Path,
        command: RawCommand,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn_inner(supervisor, command, None, false, false, false)
    }
    fn spawn_inner(
        supervisor: &Path,
        command: RawCommand,
        launch_binding: Option<crate::launch_privilege::NativeLaunchBinding>,
        deny_assignment: bool,
        skip_release: bool,
        fail_cleanup: bool,
    ) -> std::result::Result<Self, RawSpawnError> {
        ensure_spawn(supervisor)?;
        let command = command.resolved()?;
        let specification = serde_json::to_vec(&command).map_err(anyhow::Error::from)?;
        let gate = StartupGate::new()?;
        let job = Job::new()?;
        // Prestart cleanup before acquiring a process. Drop can always transfer
        // the original process/Job/worker handles, including failed startup.
        let (reaper, incoming) = mpsc::sync_channel::<Inner>(1);
        std::thread::Builder::new()
            .name("shellfox-raw-owner".into())
            .spawn(move || {
                while let Ok(mut inner) = incoming.recv() {
                    while inner.close().is_err() {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .map_err(anyhow::Error::from)?;
        let system = crate::windows::system_directory()?;
        let child = Command::new(supervisor)
            .args([
                super::supervisor::FLAG,
                &gate.release_name,
                &gate.ready_name,
            ])
            .env_clear()
            .env(
                "SystemRoot",
                system.parent().context("Missing Windows system root.")?,
            )
            .env("PATH", &system)
            .current_dir(&system)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|_| anyhow::anyhow!("Trusted raw supervisor launch failed."))?;
        let mut process = Self {
            pid: child.id(),
            inner: Some(Inner {
                child,
                job,
                assigned: false,
                input: None,
                output: Output::default(),
                readers: vec![],
                status: None,
            }),
            reaper,
            input_ended: false,
            closed_status: None,
            stop_initiated: false,
            launch_evidence: None,
        };
        let mut pending_launch = None;
        let startup = (|| -> Result<()> {
            let inner = process.inner.as_mut().context("Missing raw owner.")?;
            ensure!(!deny_assignment, "Injected raw assignment denial.");
            let handle = inner.child.as_raw_handle().cast();
            ensure!(
                unsafe { AssignProcessToJobObject(inner.job.handle(), handle) } != 0,
                "Raw Job assignment failed."
            );
            inner.assigned = true;
            let mut contained = 0;
            ensure!(
                unsafe { IsProcessInJob(handle, inner.job.handle(), &mut contained) } != 0
                    && contained != 0,
                "Exact raw Job membership unconfirmed."
            );
            if let Some(binding) = launch_binding {
                // Borrow original handles only. Snapshot capture must not extend
                // KILL_ON_JOB_CLOSE lifetime or grant process/share authority.
                pending_launch = Some(crate::launch_privilege::capture_standard_raw_before_go(
                    binding,
                    handle,
                    inner.job.handle(),
                ));
            }
            inner.readers.push(inner.output.reader(
                inner.child.stdout.take().context("Missing raw stdout.")?,
                super::RawStream::Stdout,
            )?);
            inner.readers.push(inner.output.reader(
                inner.child.stderr.take().context("Missing raw stderr.")?,
                super::RawStream::Stderr,
            )?);
            let mut stdin = inner.child.stdin.take().context("Missing raw stdin.")?;
            // A separate startup writer handles a full bounded frame without
            // blocking the owner's deadline. It then becomes the stdin writer.
            let (writer_ready, receive_writer) = mpsc::sync_channel(1);
            let startup_writer = std::thread::Builder::new()
                .name("shellfox-raw-spec".into())
                .spawn(move || {
                    use std::io::Write;
                    let result = stdin
                        .write_all(&(specification.len() as u32).to_le_bytes())
                        .and_then(|_| stdin.write_all(&specification));
                    let _ = writer_ready.send(result.map(|()| stdin));
                })?;
            inner.readers.push(startup_writer);
            if !skip_release {
                gate.release()?;
            }
            let deadline = Instant::now() + Duration::from_millis(u64::from(STARTUP_TIMEOUT_MS));
            let mut stdin = None;
            loop {
                if stdin.is_none() {
                    match receive_writer.try_recv() {
                        Ok(result) => {
                            stdin = Some(result.context("Raw launch frame write failed.")?)
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            anyhow::bail!("Raw launch writer stopped.")
                        }
                    }
                }
                if gate.ready()? && stdin.is_some() {
                    inner.input = Some(Input::start(
                        stdin.take().context("Missing raw stdin writer.")?,
                    )?);
                    ensure!(
                        !fail_cleanup,
                        "Injected post-READY failure with retained cleanup."
                    );
                    return Ok(());
                }
                ensure!(
                    inner.child.try_wait()?.is_none(),
                    "Raw target did not reach READY."
                );
                ensure!(Instant::now() < deadline, "Raw target READY timed out.");
                std::thread::sleep(Duration::from_millis(2));
            }
        })();
        if let Err(error) = startup {
            if fail_cleanup || process.close().is_err() {
                return Err(RawSpawnError {
                    error: error.context("Raw cleanup unconfirmed; owner retained."),
                    retained: Mutex::new(Some(Box::new(process))),
                });
            }
            return Err(error.into());
        }
        process.launch_evidence = pending_launch.map(|pending| pending.confirm_ready());
        Ok(process)
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn write(&mut self, bytes: Vec<u8>, eof: bool) -> Result<WriteReceipt> {
        if self.input_ended {
            return Err(AutomationStdinClosed::known().into());
        }
        let inner = self
            .inner
            .as_ref()
            .context("Raw process owner is unavailable.")?;
        if inner.status.is_some() {
            return Err(AutomationStdinClosed::known().into());
        }
        let receipt = inner
            .input
            .as_ref()
            .ok_or(AutomationStdinUnavailable::WriterNotReady)?
            .write(bytes, eof)?;
        if eof {
            self.input_ended = true;
        }
        Ok(receipt)
    }
    pub fn release_stdin(&mut self) {
        self.input_ended = true;
        if let Some(input) = self.inner.as_ref().and_then(|i| i.input.as_ref()) {
            input.close();
        }
    }
    pub fn try_output(&self) -> Option<RawOutput> {
        self.inner.as_ref().and_then(|i| i.output.read())
    }
    /// Calls `wake` after every output frame and whenever `poll_exit` may have
    /// news, never under a lock. Fires once now for anything earlier. Call once.
    pub fn set_output_wake(&self, wake: crate::Wake) -> Result<()> {
        let inner = self
            .inner
            .as_ref()
            .context("Raw process owner is unavailable.")?;
        inner.output.set_wake(wake.clone());
        inner.job.watch(wake)
    }
    /// A status is public only after the root exited and the entire Job is empty.
    pub fn poll_exit(&mut self) -> Result<Option<i32>> {
        let Some(inner) = &mut self.inner else {
            return Ok(None);
        };
        if let Some(input) = &inner.input {
            input.cancel();
        }
        if inner.status.is_none() {
            inner.status = inner.child.try_wait()?.map(|s| s.code().unwrap_or(1));
        }
        if inner.status.is_some() {
            inner.job.terminate()?;
            if inner.job.active_processes()? == 0 {
                return Ok(inner.status);
            }
        }
        Ok(None)
    }
    pub fn exit_code(&self) -> Option<i32> {
        self.closed_status
            .or_else(|| self.inner.as_ref().and_then(|i| i.status))
    }
    pub fn close(&mut self) -> Result<()> {
        if let Some(inner) = &mut self.inner {
            inner.close()?;
            self.closed_status = inner.status;
        }
        self.inner = None;
        self.input_ended = true;
        Ok(())
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
}
fn ensure_spawn(supervisor: &Path) -> Result<()> {
    ensure!(
        supervisor.is_absolute() && supervisor.is_file(),
        "Trusted absolute raw supervisor required."
    );
    // This is the trusted host executable selected by bootstrap, not a retained
    // external helper. Cargo debug executables may legitimately be hardlinked.
    Ok(())
}
