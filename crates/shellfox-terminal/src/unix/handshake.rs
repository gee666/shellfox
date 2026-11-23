//! Owner side of the controller startup: READY, GO + command frame, START.
use anyhow::{ensure, Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// `before_go` runs after READY, before the controller may launch anything.
/// Returns the stream nonblocking, for stop requests and the exit report.
pub(super) fn release(
    mut stream: UnixStream,
    payload: &[u8],
    before_go: impl FnOnce() -> Result<()>,
) -> Result<UnixStream> {
    ensure!(
        payload.len() <= crate::raw_process::command::MAX_CONFIG_BYTES,
        "controller startup payload exceeds bound"
    );
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut ready = [0; 6];
    stream.read_exact(&mut ready)?;
    ensure!(&ready == b"READY\n", "invalid controller startup frame");
    before_go()?;
    stream.write_all(b"GO\n")?;
    stream.write_all(&(payload.len() as u32).to_le_bytes())?;
    stream.write_all(payload)?;
    let mut started = [0; 6];
    stream
        .read_exact(&mut started)
        .context("owned command failed before START")?;
    ensure!(&started == b"START\n", "owned command exec failed");
    stream.set_nonblocking(true)?;
    Ok(stream)
}
