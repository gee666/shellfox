use super::WslError;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub(super) fn bounded_command(mut command: Command, limit: usize) -> Result<Vec<u8>, WslError> {
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
                        if bytes.len() > limit || is_error && !bytes.is_empty() {
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
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut output = None;
        for _ in 0..2 {
            let (is_error, bytes) = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| WslError::Deadline)??;
            if !is_error {
                output = Some(bytes);
            }
        }
        while child.try_wait().map_err(|_| WslError::Io)?.is_none() {
            if Instant::now() >= deadline {
                return Err(WslError::Deadline);
            }
            thread::sleep(Duration::from_millis(5));
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
