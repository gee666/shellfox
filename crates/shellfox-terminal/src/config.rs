#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use crate::environment::has_nul;
use crate::{EnvironmentPolicy, ShellProfileId};
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use anyhow::{ensure, Context, Result};
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use portable_pty::{CommandBuilder, PtySize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct PtyCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
}

/// Linux NonElevating requires inherited no_new_privs. macOS checks the
/// original executable and credentials and does not constrain future execs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnixPrivilegeMode {
    #[default]
    Local,
    NonElevating,
}

#[derive(Clone, Debug)]
pub struct PtyConfig {
    pub cols: u16,
    pub rows: u16,
    /// Existing, caller-owned experiment directory. This crate never deletes it.
    pub cwd: PathBuf,
    /// Writable home/config/history root, independent of the project cwd.
    pub isolation_root: PathBuf,
    /// None selects the first available safe local profile.
    pub profile: Option<ShellProfileId>,
    pub environment: EnvironmentPolicy,
    pub unix_privilege: UnixPrivilegeMode,
    /// Additional explicit executable directories under InstalledTools policy.
    pub tool_directories: Vec<PathBuf>,
    /// Controlled fixture command, mutually exclusive with an explicit profile.
    pub command: Option<PtyCommand>,
    /// Explicit additions. Isolation and executable-policy keys cannot be overridden.
    pub env: BTreeMap<OsString, OsString>,
    /// Trusted executable calling `supervisor_entry` before all other work.
    /// None is rejected before acquiring a PTY.
    pub supervisor: Option<PathBuf>,
    /// The tab UUID, exported as SHELLFOX_TERMINAL_MARKER so the `shellfox`
    /// CLI run inside the tab can target its session. WSL tabs get it via WSLENV.
    pub terminal_marker: Option<String>,
}

impl PtyConfig {
    pub fn new(isolation_root: impl Into<PathBuf>) -> Self {
        let isolation_root = isolation_root.into();
        Self {
            cols: 80,
            rows: 24,
            cwd: isolation_root.clone(),
            isolation_root,
            profile: None,
            environment: EnvironmentPolicy::Minimal,
            unix_privilege: UnixPrivilegeMode::Local,
            tool_directories: Vec::new(),
            command: None,
            env: BTreeMap::new(),
            supervisor: None,
            terminal_marker: None,
        }
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub(crate) fn validate(&self) -> Result<()> {
        size(self.cols, self.rows)?;
        ensure!(
            self.cwd.is_absolute() && self.cwd.is_dir(),
            "PTY cwd must be an existing absolute experiment directory"
        );
        ensure!(
            self.isolation_root.is_absolute()
                && self.isolation_root.is_dir()
                && !has_nul(self.isolation_root.as_os_str()),
            "isolation root must be an existing absolute experiment directory"
        );
        ensure!(
            self.command.is_none() || self.profile.is_none(),
            "custom command and shell profile are mutually exclusive"
        );
        let supervisor = self
            .supervisor
            .as_ref()
            .context("a trusted supervisor is required")?;
        ensure!(
            supervisor.is_absolute() && supervisor.is_file(),
            "supervisor must be an absolute executable path"
        );
        if let Some(command) = &self.command {
            ensure!(
                PathBuf::from(&command.program).is_absolute(),
                "PTY command must use an absolute executable path"
            );
            ensure!(!has_nul(&command.program), "command contains NUL");
            ensure!(
                command.args.iter().all(|arg| !has_nul(arg)),
                "argument contains NUL"
            );
        }
        ensure!(
            !has_nul(self.cwd.as_os_str()) && !has_nul(supervisor.as_os_str()),
            "PTY path contains NUL"
        );
        ensure!(
            self.tool_directories.is_empty()
                || self.environment == EnvironmentPolicy::InstalledTools,
            "explicit tool directories require InstalledTools policy"
        );
        for path in &self.tool_directories {
            ensure!(
                path.is_absolute() && path.is_dir() && !has_nul(path.as_os_str()),
                "tool directory must be an existing absolute path"
            );
        }
        if let Some(marker) = &self.terminal_marker {
            ensure!(
                marker.len() == 36
                    && marker.bytes().enumerate().all(|(i, b)| {
                        if matches!(i, 8 | 13 | 18 | 23) {
                            b == b'-'
                        } else {
                            b.is_ascii_hexdigit()
                        }
                    }),
                "terminal marker must be a UUID"
            );
        }
        crate::environment::validate_extra(&self.env)
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub(crate) fn builder(&self, argv: &[OsString]) -> Result<CommandBuilder> {
        let mut command = CommandBuilder::from_argv(argv.to_vec());
        command.env_clear();
        command.cwd(crate::environment::lookup_path(self.cwd.clone()));
        #[cfg(windows)]
        use crate::environment_paths as paths;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        use crate::unix::environment as paths;
        for (key, value) in paths::build(
            &self.isolation_root,
            self.environment,
            &self.tool_directories,
        )? {
            command.env(key, value);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        if let Some(marker) = &self.terminal_marker {
            const MARKER: &str = "SHELLFOX_TERMINAL_MARKER";
            command.env(MARKER, marker);
            #[cfg(windows)]
            {
                // wsl.exe forwards only the names WSLENV lists into the guest.
                let user = self
                    .env
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case("WSLENV"))
                    .map(|(_, value)| value.to_string_lossy().into_owned());
                command.env(
                    "WSLENV",
                    forward(user.as_deref().unwrap_or_default(), MARKER),
                );
            }
        }
        Ok(command)
    }
}

/// WSLENV listing `name` once, without flags, keeping every other entry.
#[cfg(any(windows, test))]
fn forward(wslenv: &str, name: &str) -> String {
    let mut entries: Vec<&str> = wslenv
        .split(':')
        .filter(|entry| {
            let key = entry.split('/').next().unwrap_or_default();
            !entry.is_empty() && !key.eq_ignore_ascii_case(name)
        })
        .collect();
    entries.push(name);
    entries.join(":")
}

#[cfg(test)]
#[test]
fn the_marker_replaces_any_flagged_wslenv_entry() {
    let name = "SHELLFOX_TERMINAL_MARKER";
    assert_eq!(forward("", name), name);
    assert_eq!(
        forward("A/p:shellfox_terminal_marker/w::B", name),
        format!("A/p:B:{name}")
    );
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub(crate) fn size(cols: u16, rows: u16) -> Result<PtySize> {
    ensure!(
        cols > 0 && rows > 0 && cols <= i16::MAX as u16 && rows <= i16::MAX as u16,
        "PTY dimensions must be in 1..=32767"
    );
    Ok(PtySize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    })
}
