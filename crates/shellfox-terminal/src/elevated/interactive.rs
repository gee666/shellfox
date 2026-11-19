//! The same installed helper and `launch` mode used by Electron. This path
//! intentionally loads the account's PowerShell profile, unlike NativeFactory.
use super::{
    native, AdapterError, Capability, Generation, Launch, Session, StartError, UserApproval,
};
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const IMAGE: &str = "Shellfox.ElevatedTerminal.exe";

/// No renderer, setting, session directory or PATH can supply the helper path.
pub struct InteractiveHelper(PathBuf);
impl InteractiveHelper {
    pub fn bundled() -> Result<Self, AdapterError> {
        let exe = std::env::current_exe().map_err(|_| AdapterError::InvalidResource)?;
        let dir = exe.parent().ok_or(AdapterError::InvalidResource)?;
        let resources = if dir.ends_with("bin") {
            dir.join("../resources")
        } else {
            dir.join("resources")
        };
        let mut candidate = resources.join("elevated-terminal").join(IMAGE);
        if cfg!(debug_assertions) && !candidate.is_file() {
            let rid = if cfg!(target_arch = "aarch64") {
                "win-arm64"
            } else {
                "win-x64"
            };
            candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tmp/terminal-native")
                .join(rid)
                .join(IMAGE);
        }
        if !candidate.is_file() {
            return Err(AdapterError::InvalidResource);
        }
        Ok(Self(candidate))
    }

    fn command(&self, mode: &str) -> Result<(Command, File), AdapterError> {
        let path = self
            .0
            .canonicalize()
            .map_err(|_| AdapterError::InvalidResource)?;
        let hold = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .map_err(|_| AdapterError::InvalidResource)?;
        let mut command = Command::new(&path);
        command
            .arg(mode)
            .current_dir(path.parent().ok_or(AdapterError::InvalidResource)?)
            .env_clear()
            .envs(std::env::vars_os().filter(|(key, _)| {
                key.to_str()
                    .is_some_and(|key| !key.starts_with('=') && !super::launch::runtime_hook(key))
            }));
        Ok((command, hold))
    }

    pub fn probe(&self) -> Result<(), AdapterError> {
        let (command, hold) = self.command("probe")?;
        match native::probe(command, Box::new(hold), Duration::from_secs(8))? {
            Capability::RetainedV1 => Ok(()),
            Capability::IsolatedV1 => Err(AdapterError::InvalidProtocol),
        }
    }

    pub fn launch(
        self,
        approval: UserApproval,
        generation: Generation,
        config: &crate::PtyConfig,
    ) -> Result<Session, StartError> {
        let prepare = || {
            if !approval.matches(generation) {
                return Err(AdapterError::StaleGeneration);
            }
            let launch = Launch::interactive(config)?.prepare()?;
            let (command, hold) = self.command("launch")?;
            Ok((command, hold, launch))
        };
        let (command, hold, launch) = prepare().map_err(|error| StartError {
            error,
            retained: None,
        })?;
        Session::start(command, Box::new(hold), generation, launch)
    }
}
