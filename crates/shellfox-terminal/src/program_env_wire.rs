//! Private startup payload. Exactly six typed-purpose keys, never Serde/maps.
use super::{same_path, valid_text};
use anyhow::{ensure, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
pub(crate) const MAX_PACKET: usize = 16 * 1024;
pub(crate) const KEYS: [&str; 6] = [
    "SSH_ASKPASS",
    "SSH_ASKPASS_REQUIRE",
    "DISPLAY",
    "SHELLFOX_SSH_ASKPASS_ENDPOINT",
    "SHELLFOX_SSH_ASKPASS_DIRECTORY",
    "SHELLFOX_SSH_ASKPASS_TOKEN",
];
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileId {
    pub first: u64,
    pub second: u64,
}
pub(crate) struct Packet(pub Vec<u8>);
impl Drop for Packet {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            unsafe {
                std::ptr::write_volatile(byte, 0);
            }
        }
    }
}
pub(crate) struct Decoded {
    program: PathBuf,
    ids: [FileId; 2],
    values: Vec<String>,
}
impl Drop for Decoded {
    fn drop(&mut self) {
        if let Some(token) = self.values.get_mut(5) {
            // Exclusive token storage, overwritten only during destruction.
            for byte in unsafe { token.as_bytes_mut() } {
                unsafe {
                    std::ptr::write_volatile(byte, 0);
                }
            }
        }
    }
}
fn put(out: &mut Vec<u8>, value: &str) -> Result<()> {
    ensure!(
        valid_text(value),
        "Invalid private program environment field."
    );
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    ensure!(
        out.len() <= MAX_PACKET,
        "Program environment payload exceeds its bound."
    );
    Ok(())
}
pub(crate) fn encode(program: &str, ids: [FileId; 2], values: [&str; 6]) -> Result<Packet> {
    let mut packet = Packet(b"SFPE\x01\0\0\0".to_vec());
    for id in ids {
        packet.0.extend_from_slice(&id.first.to_le_bytes());
        packet.0.extend_from_slice(&id.second.to_le_bytes());
    }
    put(&mut packet.0, program)?;
    for (key, value) in KEYS.iter().zip(values) {
        put(&mut packet.0, key)?;
        put(&mut packet.0, value)?;
    }
    Ok(packet)
}
fn take<'a>(input: &mut &'a [u8]) -> Result<&'a str> {
    ensure!(input.len() >= 4, "Truncated private program environment.");
    let length = u32::from_le_bytes(input[..4].try_into()?) as usize;
    *input = &input[4..];
    ensure!(
        length > 0 && length <= 4096 && input.len() >= length,
        "Invalid private environment length."
    );
    let value = std::str::from_utf8(&input[..length])?;
    ensure!(
        valid_text(value),
        "Invalid private program environment text."
    );
    *input = &input[length..];
    Ok(value)
}
pub(crate) fn decode(packet: &[u8]) -> Result<Decoded> {
    ensure!(
        packet.len() <= MAX_PACKET && packet.len() >= 40 && &packet[..8] == b"SFPE\x01\0\0\0",
        "Invalid program environment purpose/version."
    );
    let id = |offset| -> Result<FileId> {
        Ok(FileId {
            first: u64::from_le_bytes(packet[offset..offset + 8].try_into()?),
            second: u64::from_le_bytes(packet[offset + 8..offset + 16].try_into()?),
        })
    };
    let ids = [id(8)?, id(24)?];
    let mut input = &packet[40..];
    let program = PathBuf::from(take(&mut input)?);
    let mut values = Vec::new();
    for expected in KEYS {
        ensure!(
            take(&mut input)? == expected,
            "Unapproved program environment key/order."
        );
        values.push(take(&mut input)?.to_owned());
    }
    ensure!(
        input.is_empty()
            && program.is_absolute()
            && Path::new(&values[0]).is_absolute()
            && Path::new(&values[4]).is_absolute(),
        "Invalid bound program paths."
    );
    ensure!(
        super::valid_endpoint(Path::new(&values[4]), &values[3]),
        "Invalid local askpass endpoint."
    );
    ensure!(
        values[1] == "force" && values[2] == "shellfox-askpass:0",
        "Invalid askpass mode."
    );
    ensure!(
        values[5].len() == 64 && values[5].bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid IPC capability."
    );
    Ok(Decoded {
        program,
        ids,
        values,
    })
}
impl Decoded {
    pub(crate) fn apply(&self, actual: &Path, command: &mut Command) -> Result<()> {
        let actual = std::fs::canonicalize(actual)?;
        ensure!(
            same_path(&actual, &self.program),
            "Program carrier target mismatch."
        );
        #[cfg(windows)]
        {
            super::platform::check_id(&actual, self.ids[0])?;
            super::platform::check_id(Path::new(&self.values[0]), self.ids[1])?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            for (path, expected) in [
                (actual.as_path(), self.ids[0]),
                (Path::new(&self.values[0]), self.ids[1]),
            ] {
                let metadata = std::fs::metadata(path)?;
                ensure!(
                    (metadata.dev(), metadata.ino()) == (expected.first, expected.second),
                    "Program carrier file identity changed."
                );
            }
        }
        #[cfg(any(windows, unix))]
        {
            for (key, value) in KEYS.iter().zip(&self.values) {
                command.env(key, value);
            }
            Ok(())
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (command, &self.ids);
            anyhow::bail!("Program-start native ownership gate is unavailable.")
        }
    }
}
