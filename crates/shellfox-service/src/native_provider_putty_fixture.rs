use super::{Entries, MAX_BYTES, MAX_SESSIONS};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::{collections::HashMap, io::Read, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryReply {
    ok: bool,
    value: Vec<Session>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    name: String,
    values: HashMap<String, RegistryValue>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RegistryValue {
    String(String),
    Dword(u32),
}

pub(super) fn read() -> Result<Entries> {
    let path =
        PathBuf::from(std::env::var_os("SHELLFOX_P5_PUTTY_SOURCE").context(
            "Missing private PuTTY fixture; refusing registry access in acceptance mode",
        )?);
    let root = std::env::current_dir()?.join("tmp").canonicalize()?;
    ensure!(path.is_absolute(), "PuTTY fixture path must be absolute");
    let path = path.canonicalize()?;
    ensure!(
        path.starts_with(root),
        "PuTTY fixture must be inside project tmp"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_BYTES,
        "PuTTY fixture exceeds the import limit"
    );
    let reply: RegistryReply = serde_json::from_slice(&bytes)?;
    ensure!(
        reply.ok && reply.value.len() <= MAX_SESSIONS,
        "Invalid PuTTY fixture result"
    );
    Ok(reply
        .value
        .into_iter()
        .map(|row| {
            let values = row
                .values
                .into_iter()
                .map(|(key, value)| {
                    let value = match value {
                        RegistryValue::String(value) => value,
                        RegistryValue::Dword(value) => value.to_string(),
                    };
                    (key, value)
                })
                .collect();
            (row.name, values)
        })
        .collect())
}
