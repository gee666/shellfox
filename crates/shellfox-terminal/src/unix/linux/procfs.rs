use super::super::tree::Record;
pub(super) use super::stat::parse_stat;
use anyhow::{ensure, Context, Result};
use std::fs::File;
use std::io::Read;

pub(super) fn bounded(path: impl AsRef<std::path::Path>, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(max + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= max, "procfs read bound exceeded");
    Ok(bytes)
}

pub(super) fn record(pid: u32, uid: u32) -> Result<Record> {
    let bytes = bounded(format!("/proc/{pid}/stat"), 16384)?;
    let text = std::str::from_utf8(&bytes)?;
    let row = parse_stat(text, pid)?;
    if !row.dead() {
        let status = bounded(format!("/proc/{pid}/status"), 65536)?;
        let text = std::str::from_utf8(&status)?;
        let fields = text
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .context("missing process credentials")?;
        let ids = fields
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            ids.len() == 4 && ids.iter().all(|id| *id == uid),
            "owned process credentials changed"
        );
    }
    Ok(row)
}

pub(super) fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)))
}
