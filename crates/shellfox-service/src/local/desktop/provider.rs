//! Local renderer provider operations, separately authorized by the currently
//! registered desktop token. Nothing here adds a remote Manager method.
use crate::{
    desktop_launch::DesktopClientToken,
    native::NativeAction,
    ssh,
    system::{SystemNativeRequest, SystemScope},
};
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopProviderRequest {
    pub(crate) renderer: DesktopClientToken,
    operation: Operation,
}
impl fmt::Debug for DesktopProviderRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DesktopProviderRequest([REDACTED])")
    }
}
impl DesktopProviderRequest {
    pub fn from_native(renderer: DesktopClientToken, action: NativeAction) -> anyhow::Result<Self> {
        let operation = match action {
            NativeAction::System(request) => Operation::System(request.into()),
            NativeAction::Ssh(request) => Operation::Ssh(request.into()),
            _ => anyhow::bail!("Not a desktop provider operation"),
        };
        Ok(Self { renderer, operation })
    }

    pub(crate) fn into_native(self) -> anyhow::Result<NativeAction> {
        Ok(match self.operation {
            Operation::System(request) => NativeAction::System(request.into()),
            Operation::Ssh(request) => NativeAction::Ssh(request.try_into()?),
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "provider", content = "request", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    System(SystemOperation),
    Ssh(SshOperation),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum SystemOperation {
    Status { scope: SystemScope },
    SetCli { scope: SystemScope, installed: bool },
    SetExplorer { scope: SystemScope, installed: bool },
}
impl From<SystemNativeRequest> for SystemOperation {
    fn from(value: SystemNativeRequest) -> Self {
        match value {
            SystemNativeRequest::Status { scope } => Self::Status { scope },
            SystemNativeRequest::SetCli { scope, installed } => Self::SetCli { scope, installed },
            SystemNativeRequest::SetExplorer { scope, installed } => Self::SetExplorer { scope, installed },
        }
    }
}
impl From<SystemOperation> for SystemNativeRequest {
    fn from(value: SystemOperation) -> Self {
        match value {
            SystemOperation::Status { scope } => Self::Status { scope },
            SystemOperation::SetCli { scope, installed } => Self::SetCli { scope, installed },
            SystemOperation::SetExplorer { scope, installed } => Self::SetExplorer { scope, installed },
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
enum SshOperation {
    List,
    Save(SshInput),
    Delete { id: Uuid },
    Import,
    ValidateKey { selected: PathBuf },

}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SshInput {
    id: Uuid,
    name: String,
    host: String,
    port: u16,
    user: String,
    password: SshPassword,
    key_file: Option<String>,
    remote_cwd: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "camelCase", deny_unknown_fields)]
enum SshPassword { Keep, Clear, Set(ssh::Secret) }
impl From<ssh::Input> for SshInput {
    fn from(input: ssh::Input) -> Self {
        let ssh::Input { id, name, host, port, user, password, key_file, remote_cwd } = input;
        let password = match password {
            ssh::Password::Keep => SshPassword::Keep,
            ssh::Password::Clear => SshPassword::Clear,
            ssh::Password::Set(secret) => SshPassword::Set(secret),
        };
        Self { id, name, host, port, user, password, key_file, remote_cwd }
    }
}
impl From<SshInput> for ssh::Input {
    fn from(input: SshInput) -> Self {
        let SshInput { id, name, host, port, user, password, key_file, remote_cwd } = input;
        let password = match password {
            SshPassword::Keep => ssh::Password::Keep,
            SshPassword::Clear => ssh::Password::Clear,
            SshPassword::Set(secret) => ssh::Password::Set(secret),
        };
        Self { id, name, host, port, user, password, key_file, remote_cwd }
    }
}

macro_rules! ssh_operations {
    ($($name:ident $(: $data:ident)? $({ $($field:ident),* })?),* $(,)?) => {
        impl From<ssh::Request> for SshOperation {
            fn from(value: ssh::Request) -> Self {
                match value {
                    ssh::Request::ValidateKey { selected } => Self::ValidateKey { selected: selected.path().to_owned() },
                    $(ssh::Request::$name $(($data))? $({ $($field),* })? =>
                        Self::$name $(($data.into()))? $({ $($field),* })?),*
                }
            }
        }
        impl TryFrom<SshOperation> for ssh::Request {
            type Error = anyhow::Error;
            fn try_from(value: SshOperation) -> anyhow::Result<Self> {
                Ok(match value {
                    SshOperation::ValidateKey { selected } => Self::ValidateKey { selected: ssh::SelectedKey::new(selected)? },
                    $(SshOperation::$name $(($data))? $({ $($field),* })? =>
                        Self::$name $(($data.into()))? $({ $($field),* })?),*
                })
            }
        }
    };
}
ssh_operations!(List, Save: input, Delete { id }, Import);
