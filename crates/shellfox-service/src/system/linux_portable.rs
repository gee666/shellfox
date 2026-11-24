//! The portable Linux `.tar.gz`, as in 0.9: the extracted folder holds
//! `shellfox` (desktop) and `shellfox-cli`, and each launch writes
//! `~/.local/bin/shellfox` pointing at them. File-manager menus run that
//! launcher, as they run /usr/bin/shellfox for the .deb.
use super::{
    deb::user_backend, error, linux_templates::shell_quote, resources::*, NativeSystemBackend,
    SystemEnvironment, SystemResult, SystemScope,
};
use shellfox_protocol::ErrorCode;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// First words that open the desktop instead of running the CLI, as in 0.9.
pub const DESKTOP_VERBS: [&str; 6] = [
    "start",
    "add",
    "new",
    "create",
    "--new-session",
    "--new-window",
];

/// POSIX launcher tail; the script sets `$exe` (desktop) and `$cli` first.
pub fn posix_router() -> String {
    format!(
        "case \"${{1:-}}\" in\n  {})\n    \"$exe\" \"$@\" </dev/null >/dev/null 2>&1 &\n    exit 0\n    ;;\nesac\nexec \"$cli\" \"$@\"\n",
        DESKTOP_VERBS.join("|")
    )
}

pub struct PortableLinux {
    root: PathBuf,
    bin: PathBuf,
}
impl PortableLinux {
    /// The extracted archive, when `desktop` is its `shellfox` binary with
    /// `shellfox-cli` beside it. The caller rules out development builds.
    pub fn current(desktop: &Path, environment: &dyn SystemEnvironment) -> Option<Self> {
        let root = desktop.parent()?;
        if desktop.file_name()? != "shellfox" || !root.join("shellfox-cli").is_file() {
            return None;
        }
        let bin = environment.home().ok()?.join(".local/bin");
        Some(Self::at(root.to_owned(), bin))
    }
    pub fn at(root: PathBuf, bin: PathBuf) -> Self {
        Self { root, bin }
    }
    fn desktop(&self) -> PathBuf {
        self.root.join("shellfox")
    }
    fn cli(&self) -> PathBuf {
        self.root.join("shellfox-cli")
    }
    fn launcher(&self) -> PathBuf {
        self.bin.join("shellfox")
    }
    /// Per-user launcher and menus, against the given HOME/XDG environment.
    pub fn backend(self, environment: Arc<dyn SystemEnvironment>) -> NativeSystemBackend {
        user_backend(Arc::new(self), environment)
    }
    fn script(&self) -> SystemResult<String> {
        let quote = |path: PathBuf| {
            path.to_str()
                .filter(|text| !text.contains(['\n', '\r']))
                .map(shell_quote)
                .ok_or_else(|| {
                    error(
                        ErrorCode::Unsupported,
                        "The Shellfox folder name can't be used in a launcher.",
                    )
                })
        };
        Ok(format!(
            "#!/bin/sh\n# {POSIX_OWNER}\nexe={}\ncli={}\n{}",
            quote(self.desktop())?,
            quote(self.cli())?,
            posix_router()
        ))
    }
}
impl SystemInstallResources for PortableLinux {
    fn cli_plan(&self, scope: SystemScope) -> SystemResult<CliInstallPlan> {
        if scope == SystemScope::System {
            return Err(error(
                ErrorCode::Unsupported,
                "The portable app installs only the per-user CLI.",
            ));
        }
        let launcher = self.launcher();
        Ok(CliInstallPlan {
            directory: self.bin.clone(),
            files: vec![ManagedFile {
                path: launcher.clone(),
                bytes: self.script()?.into_bytes(),
                executable: true,
            }],
            marker: launcher,
            package_managed: false,
        })
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        Ok(DesktopLaunch {
            executable: self.desktop(),
            prefix_arguments: Vec::new(),
            new_session_contract: true,
            disposition: DesktopLaunchDisposition::ExistingWindow,
            cli_start: Some(self.launcher()),
        })
    }
    fn validate_current(&self) -> SystemResult<()> {
        if self.desktop().is_file() && self.cli().is_file() {
            Ok(())
        } else {
            Err(error(
                ErrorCode::NativeUnavailable,
                "The extracted Shellfox folder is incomplete.",
            ))
        }
    }
}
