use crate::unix::Capability;
use anyhow::{ensure, Context, Result};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn check(helper: &Path, arch: &str) -> Result<()> {
    let mut child = Command::new(helper)
        .arg("--capabilities")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .context("capability output pipe missing")?;
        // SAFETY: descriptor is valid for this scope. O_NONBLOCK is a valid flag.
        let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
        ensure!(
            flags >= 0
                && unsafe {
                    libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                } == 0,
            "cannot bound helper preflight reads"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut bytes = Vec::new();
        let mut eof = false;
        loop {
            let mut chunk = [0u8; 512];
            match stdout.read(&mut chunk) {
                Ok(0) => eof = true,
                Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
            ensure!(
                bytes.len() <= 2048,
                "supervisor capability exceeds frame bound"
            );
            if let Some(status) = child.try_wait()? {
                ensure!(status.success(), "supervisor capability process failed");
                if eof {
                    Capability::parse(&bytes, arch)?;
                    return Ok(());
                }
            }
            ensure!(Instant::now() < deadline, "supervisor preflight timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        // Only our unreaped direct capability child. Never a shell target PID.
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
