#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use crate::environment::has_nul;
use crate::{EnvironmentPolicy, ShellProfileId, TerminalEnvironment};
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
    /// Existing working directory. This crate never deletes it.
    pub cwd: PathBuf,
    /// None selects the first available safe local profile.
    pub profile: Option<ShellProfileId>,
    pub environment: TerminalEnvironment,
    pub unix_privilege: UnixPrivilegeMode,
    /// Controlled fixture command, mutually exclusive with an explicit profile.
    pub command: Option<PtyCommand>,
    /// Settings and session variables, applied over the base environment.
    pub env: BTreeMap<OsString, OsString>,
    /// The installed Shellfox CLI launcher directory, put first on PATH.
    pub cli_bin: Option<PathBuf>,
    /// Host-owned paths such as SHELLFOX_USER_DATA, set last. WSL gets them translated.
    pub host_paths: BTreeMap<OsString, OsString>,
    /// Trusted executable calling `supervisor_entry` before all other work.
    /// None is rejected before acquiring a PTY.
    pub supervisor: Option<PathBuf>,
    /// The tab UUID, exported as SHELLFOX_TERMINAL_MARKER so the `shellfox`
    /// CLI run inside the tab can target its session. WSL tabs get it via WSLENV.
    pub terminal_marker: Option<String>,
}

impl PtyConfig {
    /// A user terminal: the user's own environment, starting in `cwd`.
    pub fn user(cwd: impl Into<PathBuf>) -> Self {
        Self::with(cwd.into(), TerminalEnvironment::User)
    }
    /// A test or fixture terminal whose state lives under `root`, which is also its cwd.
    pub fn isolated(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let environment = TerminalEnvironment::Isolated {
            root: root.clone(),
            policy: EnvironmentPolicy::Minimal,
            tool_directories: Vec::new(),
        };
        Self::with(root, environment)
    }
    fn with(cwd: PathBuf, environment: TerminalEnvironment) -> Self {
        Self {
            cols: 80,
            rows: 24,
            cwd,
            profile: None,
            environment,
            unix_privilege: UnixPrivilegeMode::Local,
            command: None,
            env: BTreeMap::new(),
            cli_bin: None,
            host_paths: BTreeMap::new(),
            supervisor: None,
            terminal_marker: None,
        }
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub(crate) fn validate(&self) -> Result<()> {
        size(self.cols, self.rows)?;
        ensure!(
            self.cwd.is_absolute() && self.cwd.is_dir(),
            "PTY cwd must be an existing absolute directory"
        );
        if let TerminalEnvironment::Isolated {
            root,
            policy,
            tool_directories,
        } = &self.environment
        {
            ensure!(
                root.is_absolute() && root.is_dir() && !has_nul(root.as_os_str()),
                "isolation root must be an existing absolute directory"
            );
            ensure!(
                tool_directories.is_empty() || *policy == EnvironmentPolicy::InstalledTools,
                "explicit tool directories require InstalledTools policy"
            );
            for path in tool_directories {
                ensure!(
                    path.is_absolute() && path.is_dir() && !has_nul(path.as_os_str()),
                    "tool directory must be an existing absolute path"
                );
            }
        }
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
        if let Some(bin) = &self.cli_bin {
            ensure!(
                bin.is_absolute() && !has_nul(bin.as_os_str()),
                "CLI directory must be an absolute path"
            );
        }
        crate::environment::validate_extra(&self.host_paths)?;
        crate::environment::validate_extra(&self.env)
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub(crate) fn builder(&self, argv: &[OsString]) -> Result<CommandBuilder> {
        let mut command = CommandBuilder::from_argv(argv.to_vec());
        command.env_clear();
        command.cwd(crate::environment::lookup_path(self.cwd.clone()));
        for (key, value) in self.environment_entries()? {
            command.env(key, value);
        }
        Ok(command)
    }

    /// The complete child environment.
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub(crate) fn environment_entries(&self) -> Result<Vec<(OsString, OsString)>> {
        #[cfg(windows)]
        use crate::environment_paths as paths;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        use crate::unix::environment as paths;
        let base = match &self.environment {
            TerminalEnvironment::User => crate::environment_user::user_base(),
            TerminalEnvironment::Isolated {
                root,
                policy,
                tool_directories,
            } => paths::build(root, *policy, tool_directories)?
                .into_iter()
                .collect(),
        };
        Ok(crate::environment_user::compose(
            base,
            crate::environment_user::Layers {
                configured: Some(&self.env),
                cli_bin: self.cli_bin.as_deref(),
                marker: self.terminal_marker.as_deref(),
                host_paths: Some(&self.host_paths),
            },
        ))
    }
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
