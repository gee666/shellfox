//! Native-bootstrap-only program environment. No map, wire API, or profile authority.
use anyhow::{ensure, Result};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};
#[cfg(windows)]
#[path = "program_env_windows.rs"]
mod platform;
#[path = "program_env_wire.rs"]
pub(crate) mod wire;

/// Implement only in trusted bootstrap code retaining actual verified resources.
/// No permissive implementation/default is supplied and no request deserializes it.
pub type ProgramLeaseResult = anyhow::Result<()>;
pub trait TrustedProgramLease: Send + Sync {
    fn path(&self) -> &Path;
    fn revalidate(&self) -> ProgramLeaseResult;
}
pub struct AskpassBroker {
    directory: PathBuf,
    endpoint: String,
    token: Vec<u8>,
}
impl std::fmt::Debug for AskpassBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AskpassBroker([redacted])")
    }
}
impl Drop for AskpassBroker {
    fn drop(&mut self) {
        for byte in &mut self.token {
            unsafe {
                std::ptr::write_volatile(byte, 0);
            }
        }
    }
}
impl AskpassBroker {
    pub fn new(directory: PathBuf, endpoint: String, token: String) -> Result<Self> {
        ensure!(
            directory.is_absolute() && directory.is_dir(),
            "Askpass directory must be an existing absolute path."
        );
        ensure!(
            valid_text(directory.as_os_str().to_str().unwrap_or("")) && valid_text(&endpoint),
            "Invalid askpass endpoint/directory."
        );
        ensure!(
            token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid askpass IPC capability."
        );
        ensure!(
            valid_endpoint(&directory, &endpoint),
            "Askpass endpoint escaped its local scope."
        );
        Ok(Self {
            directory,
            endpoint,
            token: token.into_bytes(),
        })
    }
}
struct BoundProgram {
    lease: Arc<dyn TrustedProgramLease>,
    path: PathBuf,
    file: File,
}
impl BoundProgram {
    fn new(lease: Arc<dyn TrustedProgramLease>) -> Result<Self> {
        lease
            .revalidate()
            .map_err(|_| anyhow::anyhow!("Trusted program lease is invalid."))?;
        ensure!(
            lease.path().is_absolute() && lease.path().is_file(),
            "Trusted program must be an absolute file."
        );
        let path = std::fs::canonicalize(lease.path())?;
        #[cfg(windows)]
        let file = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&path)?
        };
        #[cfg(not(windows))]
        let file = File::open(&path)?;
        let bound = Self { lease, path, file };
        bound.revalidate()?;
        Ok(bound)
    }
    fn revalidate(&self) -> Result<()> {
        self.lease
            .revalidate()
            .map_err(|_| anyhow::anyhow!("Trusted program lease is invalid."))?;
        ensure!(
            same_path(&std::fs::canonicalize(self.lease.path())?, &self.path),
            "Trusted program lease binding changed."
        );
        #[cfg(windows)]
        {
            platform::same_file(&self.path, &self.file)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = self.file.metadata()?;
            let now = std::fs::metadata(&self.path)?;
            ensure!(
                (held.dev(), held.ino()) == (now.dev(), now.ino()),
                "Trusted program identity changed."
            );
        }
        Ok(())
    }
}
/// Opaque, non-Serde and redacted. Holds target AND askpass helper leases until
/// owned cleanup. It never authorizes shell profiles, elevation, WSL or raw stdin.
pub struct TrustedProgramEnv {
    program: BoundProgram,
    askpass: BoundProgram,
    broker: AskpassBroker,
}
impl std::fmt::Debug for TrustedProgramEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrustedProgramEnv([redacted])")
    }
}
impl TrustedProgramEnv {
    pub fn ssh_askpass(
        program: Arc<dyn TrustedProgramLease>,
        askpass: Arc<dyn TrustedProgramLease>,
        broker: AskpassBroker,
    ) -> Result<Self> {
        Ok(Self {
            program: BoundProgram::new(program)?,
            askpass: BoundProgram::new(askpass)?,
            broker,
        })
    }
    pub(crate) fn validate_for(&self, config: &crate::PtyConfig) -> Result<()> {
        ensure!(
            config.profile.is_none(),
            "Trusted program environment cannot authorize a shell profile."
        );
        let command = config
            .command
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Trusted environment requires an explicit program."))?;
        self.program.revalidate()?;
        self.askpass.revalidate()?;
        let actual = std::fs::canonicalize(PathBuf::from(&command.program))?;
        ensure!(
            same_path(&actual, &self.program.path),
            "Trusted environment belongs to a different program."
        );
        ensure!(
            !config.env.keys().any(|key| wire::KEYS
                .iter()
                .any(|name| key.to_string_lossy().eq_ignore_ascii_case(name))),
            "Program carrier keys cannot be supplied through user environment."
        );
        Ok(())
    }
    pub(crate) fn packet(&self) -> Result<wire::Packet> {
        self.program.revalidate()?;
        self.askpass.revalidate()?;
        let program = self
            .program
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Unsupported trusted program path."))?;
        let askpass = self
            .askpass
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Unsupported askpass path."))?;
        let directory = self
            .broker
            .directory
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Unsupported broker directory."))?;
        let token = std::str::from_utf8(&self.broker.token)
            .map_err(|_| anyhow::anyhow!("Invalid broker capability."))?;
        #[cfg(windows)]
        let ids = [
            platform::file_id(&self.program.file)?,
            platform::file_id(&self.askpass.file)?,
        ];
        #[cfg(unix)]
        let ids = {
            use std::os::unix::fs::MetadataExt;
            let one = self.program.file.metadata()?;
            let two = self.askpass.file.metadata()?;
            [
                wire::FileId {
                    first: one.dev(),
                    second: one.ino(),
                },
                wire::FileId {
                    first: two.dev(),
                    second: two.ino(),
                },
            ]
        };
        wire::encode(
            program,
            ids,
            [
                askpass,
                "force",
                "shellfox-askpass:0",
                &self.broker.endpoint,
                directory,
                token,
            ],
        )
    }
}
fn valid_endpoint(directory: &Path, endpoint: &str) -> bool {
    #[cfg(windows)]
    {
        let _ = directory;
        endpoint.starts_with(r"\\.\pipe\shellfox-rust-v1-") && endpoint.len() <= 256
    }
    #[cfg(unix)]
    {
        Path::new(endpoint).is_absolute() && Path::new(endpoint).parent() == Some(directory)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (directory, endpoint);
        false
    }
}
fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}
fn same_path(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

#[cfg(test)]
#[path = "program_env_tests.rs"]
mod tests;
