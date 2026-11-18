use super::WslError;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslProfileId(String);
impl WslProfileId {
    pub fn parse(id: &str) -> Result<Self, WslError> {
        let name = id.strip_prefix("wsl:").ok_or(WslError::InvalidRequest)?;
        if name.is_empty()
            || name.len() > 160
            || name.trim() != name
            || name.chars().any(char::is_control)
        {
            return Err(WslError::InvalidRequest);
        }
        Ok(Self(name.into()))
    }
    pub fn distro(&self) -> &str {
        &self.0
    }
    pub fn id(&self) -> String {
        format!("wsl:{}", self.0)
    }
}
/// Absolute OS-provided wsl.exe only. Never searches PATH or a user override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsWsl(PathBuf);
impl OsWsl {
    pub fn discover() -> Result<Self, WslError> {
        #[cfg(windows)]
        {
            let path = crate::windows::system_directory()
                .map_err(|_| WslError::Unavailable)?
                .join("wsl.exe");
            if !path.is_file()
                || std::fs::symlink_metadata(&path)
                    .map_err(|_| WslError::Unavailable)?
                    .file_type()
                    .is_symlink()
            {
                return Err(WslError::Unavailable);
            }
            Ok(Self(path))
        }
        #[cfg(not(windows))]
        {
            Err(WslError::Unavailable)
        }
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    #[cfg(feature = "test-support")]
    pub fn fixture(path: PathBuf) -> Result<Self, WslError> {
        if !path.is_absolute() || !path.is_file() {
            return Err(WslError::InvalidRequest);
        }
        Ok(Self(path))
    }
    /// This only lists installed distro names. It does not start a guest.
    pub fn list(&self) -> Result<Vec<WslProfileId>, WslError> {
        let output = super::discovery_process::bounded_command(
            self.command(&["--list", "--quiet"]),
            64 * 1024,
        )?;
        decode_distro_list(&output)
    }
    pub(crate) fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.0);
        command.args(args);
        // Do not transfer ambient marker, credentials or custom loader settings.
        command.env_remove("WSLENV");
        command.env_remove("SHELLFOX_TERMINAL_MARKER");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }
    pub(crate) fn guest_command(
        &self,
        distro: &WslProfileId,
        settings: &super::GuestSettings,
    ) -> Command {
        let mut command = self.command(&[
            "--distribution",
            distro.distro(),
            "--exec",
            &settings.python,
            "-I",
            "-S",
            "-c",
            super::BOOTSTRAP,
        ]);
        command.arg(settings.artifacts().to_string());
        command
    }
}
/// wsl.exe emits UTF-16LE, including non-ASCII distro names. Invalid encoding,
/// duplicates, control characters and oversized lists fail closed.
pub fn decode_distro_list(bytes: &[u8]) -> Result<Vec<WslProfileId>, WslError> {
    if bytes.len() > 64 * 1024 || !bytes.len().is_multiple_of(2) {
        return Err(WslError::Protocol);
    }
    let words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let text = String::from_utf16(&words).map_err(|_| WslError::Protocol)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut result = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let profile =
            WslProfileId::parse(&format!("wsl:{line}")).map_err(|_| WslError::Protocol)?;
        if result.contains(&profile) || result.len() >= 32 {
            return Err(WslError::Protocol);
        }
        result.push(profile);
    }
    Ok(result)
}
