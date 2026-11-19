use super::WslError;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub(super) fn bounded_command(command: Command, limit: usize) -> Result<Vec<u8>, WslError> {
    run(command, limit, true, || false)
}

#[cfg(windows)]
pub(super) fn profile_command(
    command: Command,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, WslError> {
    // Node's execFile allows stderr within its independent byte budget.
    run(command, 1024 * 1024, false, cancelled)
}

fn run(
    mut command: Command,
    limit: usize,
    quiet: bool,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, WslError> {
    if cancelled() {
        return Err(WslError::Unavailable);
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    #[cfg(windows)]
    let job = super::windows::TransportJob::new()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| WslError::Unavailable)?;
    #[cfg(windows)]
    if let Err(error) = job.assign(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let stdout = child.stdout.take().ok_or(WslError::Io)?;
    let stderr = child.stderr.take().ok_or(WslError::Io)?;
    let (tx, rx) = mpsc::sync_channel(2);
    for (stream, is_error) in [
        (Box::new(stdout) as Box<dyn Read + Send>, false),
        (Box::new(stderr) as Box<dyn Read + Send>, true),
    ] {
        let tx = tx.clone();
        if thread::Builder::new()
            .name("wsl-bounded-discovery".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let result = stream
                    .take(limit as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| WslError::Io)
                    .and({
                        if bytes.len() > limit || quiet && is_error && !bytes.is_empty() {
                            Err(WslError::Protocol)
                        } else {
                            Ok((is_error, bytes))
                        }
                    });
                let _ = tx.send(result);
            })
            .is_err()
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(WslError::Unavailable);
        }
    }
    let result = (|| {
        let remaining = || {
            if cancelled() {
                return Err(WslError::Unavailable);
            }
            deadline
                .checked_duration_since(Instant::now())
                .ok_or(WslError::Deadline)
        };
        let mut output = None;
        for _ in 0..2 {
            let (is_error, bytes) = loop {
                match rx.recv_timeout(remaining()?.min(Duration::from_millis(25))) {
                    Ok(result) => break result?,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Err(WslError::Io),
                }
            };
            if !is_error {
                output = Some(bytes);
            }
        }
        while child.try_wait().map_err(|_| WslError::Io)?.is_none() {
            thread::sleep(remaining()?.min(Duration::from_millis(5)));
        }
        if !child.wait().map_err(|_| WslError::Io)?.success() {
            return Err(WslError::Unavailable);
        }
        output.ok_or(WslError::Protocol)
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    #[cfg(windows)]
    job.finish()?;
    result
}
