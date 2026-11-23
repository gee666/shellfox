//! Controller side of the startup handshake and the guest exit report.
use crate::raw_process::RawCommand;
use anyhow::{ensure, Result};
use std::{
    io::{Read, Write},
    os::unix::{net::UnixStream, process::ExitStatusExt},
    process::ExitStatus,
    time::Duration,
};

/// READY, then the owner's GO and the target command frame.
pub(super) fn released_command(stream: &mut UnixStream) -> Result<RawCommand> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(b"READY\n")?;
    let mut go = [0; 3];
    stream.read_exact(&mut go)?;
    ensure!(&go == b"GO\n", "Raw controller not released");
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(
        (1..=crate::raw_process::command::MAX_CONFIG_BYTES).contains(&length),
        "Raw frame bound exceeded"
    );
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    let command: RawCommand =
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("Invalid raw startup frame"))?;
    command.validate()?;
    ensure!(
        std::path::Path::new(&command.executable).is_absolute(),
        "Raw executable not resolved"
    );
    Ok(command)
}

/// The original guest's own wait status, never controller cleanup, decides
/// natural exit, signal 128+n and the SIGKILL distinction.
pub(super) fn exit_report(status: ExitStatus) -> Vec<u8> {
    let code = status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1));
    let mut frame = code.to_le_bytes().to_vec();
    frame.push(u8::from(status.signal() == Some(libc::SIGKILL)));
    frame
}
