use serde::{Deserialize, Serialize};
use shellfox_protocol::{AppError, CliIntegration, Directory, ExplorerIntegration};
use std::path::PathBuf;

pub type SystemResult<T> = Result<T, AppError>;

/// Per-user state never implies system-wide installation or authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemScope {
    User,
    System,
}

/// Only explicit local owner requests may dispatch these actions. Facts and boot
/// call the read-only query methods, never this mutation/opening path.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SystemAction {
    SetCli { installed: bool, scope: SystemScope },
    SetExplorer { installed: bool, scope: SystemScope },
    OpenDirectory { path: PathBuf },
    OpenWeb { url: String },
}

impl SystemAction {
    pub fn validate(&self) -> SystemResult<()> {
        match self {
            Self::OpenDirectory { path } => super::validation::absolute_path(path),
            Self::OpenWeb { url } => super::validation::web_url(url).map(|_| ()),
            Self::SetCli { .. } | Self::SetExplorer { .. } => Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
pub enum SystemResponse {
    Cli(CliIntegration),
    Explorer(ExplorerIntegration),
    Opened,
}

/// Independent query failures remain failures. A missing launcher proven by a
/// successful filesystem query is not the same as unavailable discovery.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemFacts {
    pub home: SystemResult<Directory>,
    pub cli: SystemResult<CliIntegration>,
    pub explorer: SystemResult<ExplorerIntegration>,
}

/// Injectable host provider. Implementations must not mutate installation,
/// PATH, profiles or files in any of the three query methods.
/// These methods are synchronous; the facts adapter runs them off-owner.
pub trait SystemBackend: Send + Sync {
    fn home_directory(&self) -> SystemResult<Directory>;
    fn cli_integration(&self, scope: SystemScope) -> SystemResult<CliIntegration>;
    fn explorer_integration(&self, scope: SystemScope) -> SystemResult<ExplorerIntegration>;
    fn perform_user_action(&self, action: SystemAction) -> SystemResult<SystemResponse>;
}
