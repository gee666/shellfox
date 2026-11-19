//! Private in-process host contracts. No Serialize/Deserialize or wire method.
use super::{SystemFacts, SystemResult, SystemScope};
use shellfox_protocol::{AppError, CliIntegration, ExplorerIntegration};

#[derive(Clone, Debug)]
pub enum SystemNativeRequest {
    Status { scope: SystemScope },
    SetCli { scope: SystemScope, installed: bool },
    SetExplorer { scope: SystemScope, installed: bool },
}
impl SystemNativeRequest {
    /// Closed bool/scope variants have no strings/paths/commands to parse.
    /// This is syntax only, not authority, consent, resource or policy proof.
    pub fn validate(&self) -> SystemResult<()> {
        Ok(())
    }
    pub fn is_mutation(&self) -> bool {
        !matches!(self, Self::Status { .. })
    }
    pub fn scope(&self) -> SystemScope {
        match self {
            Self::Status { scope }
            | Self::SetCli { scope, .. }
            | Self::SetExplorer { scope, .. } => *scope,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SystemCapability {
    Available,
    Unavailable(AppError),
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemMutationCapabilities {
    pub install: SystemCapability,
    pub uninstall: SystemCapability,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemNativeCapabilities {
    pub cli: SystemMutationCapabilities,
    pub explorer: SystemMutationCapabilities,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemNativeStatus {
    pub scope: SystemScope,
    pub facts: SystemFacts,
    pub capabilities: SystemNativeCapabilities,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SystemNativeResponse {
    Status(Box<SystemNativeStatus>),
    Cli(CliIntegration),
    Explorer(ExplorerIntegration),
}

/// Must validate the original active LOCAL/native controller on the same owner,
/// not a remote client UUID, terminal input lease, PID or renderer-supplied bit.
/// The owner must not dispatch system actions through its remote allowlist.
pub trait SystemNativeAuthority {
    fn validate_system_controller(&self, controller: u64) -> SystemResult<()>;
}
