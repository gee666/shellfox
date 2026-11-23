use anyhow::{ensure, Result};
use std::time::Duration;
mod owned;
pub use owned::OwnedHost;

pub fn settings(
    cli: &std::path::Path,
    data: &std::path::Path,
    helper: &std::path::Path,
    digest: [u8; 32],
    words: &[&str],
) -> Result<shellfox_core::Settings> {
    use std::{
        io::Read,
        process::{Command, Stdio},
    };
    let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut command = OwnedHost(
        Command::new(cli)
            .arg("--data-dir")
            .arg(data)
            .arg("--helper")
            .arg(helper)
            .arg("--helper-sha256")
            .arg(hash)
            .args(words)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let stdout = command
        .0
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Missing CLI stdout."))?;
    let reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stdout.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let status = command.wait(Duration::from_secs(40))?;
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("CLI stdout reader failed."))??;
    ensure!(status.success(), "Settings CLI command failed.");
    ensure!(
        bytes.len() <= 8 * 1024 * 1024,
        "Settings CLI output exceeded limit."
    );
    let reply: serde_json::Value = serde_json::from_slice(&bytes)?;
    ensure!(reply["ok"] == true, "Settings CLI command failed: {reply}");
    let settings: shellfox_core::Settings = serde_json::from_value(reply["value"].clone())?;
    settings.validate()?;
    Ok(settings)
}
