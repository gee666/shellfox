use crate::{Error, Result};
use std::{
    ffi::OsString,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// No shell, no inherited stdin and no command-line passwords.
#[derive(Clone, Debug)]
pub struct ProcessRequest {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub timeout: Duration,
    pub max_output: usize,
}
#[derive(Debug)]
pub struct ProcessOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
pub trait ProcessRunner: Send + Sync {
    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput>;
}
pub struct SystemProcessRunner;
impl ProcessRunner for SystemProcessRunner {
    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput> {
        if !request.executable.is_absolute()
            || request.timeout.is_zero()
            || request.timeout > Duration::from_secs(120)
            || request.max_output > 1024 * 1024
        {
            return Err(Error::Process);
        }
        let mut command = Command::new(&request.executable);
        command
            .args(&request.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|_| Error::Process)?;
        let stdout = child.stdout.take().ok_or(Error::Process)?;
        let stderr = child.stderr.take().ok_or(Error::Process)?;
        let collect = |pipe: Box<dyn Read + Send>| {
            let (tx, rx) = mpsc::sync_channel(1);
            let limit = request.max_output;
            thread::spawn(move || {
                let mut bytes = Vec::new();
                let result = pipe.take(limit as u64 + 1).read_to_end(&mut bytes);
                let _ = tx.send(if result.is_ok() && bytes.len() <= limit {
                    Ok(bytes)
                } else {
                    Err(Error::Process)
                });
            });
            rx
        };
        let out = collect(Box::new(stdout));
        let err = collect(Box::new(stderr));
        let deadline = Instant::now() + request.timeout;
        let receive = |rx: mpsc::Receiver<Result<Vec<u8>>>| {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| Error::Process)?
        };
        // Both pipes close when the child exits, so waiting on them needs no polling.
        let output = receive(out).and_then(|stdout| Ok((stdout, receive(err)?)));
        let status = match output {
            Ok(_) => loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    // Pipes closed but the process lingers (it detached them).
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    _ => break None,
                }
            },
            Err(_) => None,
        };
        let (Some(status), Ok((stdout, stderr))) = (status, output) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Process);
        };
        Ok(ProcessOutput {
            success: status.success(),
            stdout,
            stderr,
        })
    }
}
