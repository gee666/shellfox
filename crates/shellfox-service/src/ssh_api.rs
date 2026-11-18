//! Host-only native contracts. None of the request/credential types are wire DTOs.
use anyhow::Result;
use shellfox_core::TabId;
use shellfox_runtime::Identity;
use shellfox_ssh::{ImportResult, ImportSession, ProfileDto, Secret, TrustDecision};
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

/// Deliberately not Debug or Serialize. Empty Set is distinct from Keep/Clear.
#[derive(Clone)]
pub enum Password {
    Keep,
    Clear,
    Set(Secret),
}
impl Password {
    pub fn set(value: String) -> Result<Self> {
        Ok(Self::Set(Secret::new(value)?))
    }
}
#[derive(Clone)]
pub struct Input {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Password,
    pub key_file: Option<String>,
    pub remote_cwd: Option<String>,
}
#[derive(Clone)]
pub enum Request {
    List,
    Save(Input),
    Delete {
        id: Uuid,
    },
    Import,
    ValidateKey {
        selected: SelectedKey,
    },
    GetStatus {
        connection: Option<Uuid>,
    },
    Connect {
        profile: Uuid,
        tab: TabId,
        cols: u16,
        rows: u16,
    },
    ConfirmHost {
        connection: Uuid,
        challenge: Uuid,
        decision: TrustDecision,
    },
    AnswerPrompt {
        connection: Uuid,
        prompt: Uuid,
        answer: Option<Secret>,
    },
    Disconnect {
        connection: Uuid,
    },
    AdoptReady {
        connection: Uuid,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostChallenge {
    pub challenge: Uuid,
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub prompt: Uuid,
    pub label: String,
    pub echo: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Preparing,
    AwaitingHost(HostChallenge),
    AwaitingPrompt(Prompt),
    Starting,
    Authenticating,
    Ready,
    Closing,
    Closed,
    Failed { message: String },
}
#[derive(Debug, Clone)]
pub struct ConnectionStatus {
    /// Native owner projection only: exact current registered runtime, never intent.
    pub runtime: Option<Identity>,
    pub connection: Uuid,
    pub profile: Uuid,
    pub tab: TabId,
    pub state: State,
}
#[derive(Debug)]
pub enum Response {
    Profiles(Vec<ProfileDto>),
    Imported(ImportResult),
    SelectedKey(SelectedKeyInfo),
    Status(Vec<ConnectionStatus>),
    Applied,
}
/// Native application authority is separate from terminal input/control leases.
/// A pending connection need not own an input lease, and takeover never revokes SSH.
pub trait NativeControllerValidator {
    fn validate_native(&self, controller: u64) -> Result<()>;
    /// Check tab existence/application permission, NOT the current PTY input owner.
    fn validate_target(&self, controller: u64, tab: TabId) -> Result<()>;
}
/// A unique SSH source reservation. Only the manager constructs it, before preparation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceConnectTarget {
    connection: Uuid,
    tab: TabId,
}
impl SourceConnectTarget {
    pub(super) fn new(connection: Uuid, tab: TabId) -> Self {
        Self { connection, tab }
    }
    pub fn connection(self) -> Uuid {
        self.connection
    }
    pub fn tab(self) -> TabId {
        self.tab
    }
}
pub trait NativePlatform: Send + Sync {
    /// Read-only native import discovery. Interactive dialogs belong to the local UI.
    fn import_sessions(&self) -> Result<Vec<ImportSession>>;
}
/// Minted by an explicit local UI selection, never deserialized from remote/local wire.
#[derive(Clone)]
pub struct SelectedKey(PathBuf);
impl SelectedKey {
    pub fn new(path: PathBuf) -> Result<Self> {
        anyhow::ensure!(
            path.is_absolute()
                && path
                    .to_str()
                    .is_some_and(|s| s.len() <= 32767 && !s.chars().any(char::is_control)),
            "Invalid selected SSH key path."
        );
        Ok(Self(path))
    }
    pub(super) fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl std::fmt::Debug for SelectedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SelectedKey([HOST-ONLY])")
    }
}
#[derive(Clone, Debug)]
pub struct SelectedKeyInfo {
    pub path: String,
    pub metadata: shellfox_ssh::key_metadata::KeyMetadata,
    pub cryptographically_validated: bool,
}
/// Native owner must install this against its existing runtime registry/Host.
/// This interface cannot return a second parser, raw SshSession or unrelated Runtime.
pub trait OwnedRuntimeStarter {
    /// Atomically reject an existing local runtime/other source and mark this tab
    /// SSH-owned. Local attach/start paths must consult this SAME reservation map.
    fn reserve_target(&mut self, controller: u64, target: SourceConnectTarget) -> Result<()>;
    /// Release only the exact source, after worker/broker/runtime cleanup is confirmed.
    fn release_target(&mut self, target: SourceConnectTarget) -> Result<()>;
    /// Read-only generation/liveness check. Must not claim input or change its epoch.
    fn runtime_is_current(&self, identity: Identity) -> Result<bool>;
    /// Register a generation before launch. Err means NO runtime was registered/spawned.
    /// If a started runtime is draining or failed, return its Identity for retained cleanup.
    fn start(
        &mut self,
        controller: u64,
        target: SourceConnectTarget,
        launch: &shellfox_ssh::system::SshLaunchRequest,
        local_environment: &BTreeMap<String, String>,
    ) -> Result<Identity>;
    /// Host cleanup authority, independent of a disconnected/revoked input controller.
    /// true means the original owned process tree is confirmed closed.
    fn close(&mut self, identity: Identity) -> Result<bool>;
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::List => "List",
            Self::Save(_) => "Save([HOST-ONLY])",
            Self::Delete { .. } => "Delete",
            Self::Import => "Import",
            Self::ValidateKey { .. } => "ValidateKey([HOST-ONLY])",
            Self::GetStatus { .. } => "GetStatus",
            Self::Connect { .. } => "Connect",
            Self::ConfirmHost { .. } => "ConfirmHost",
            Self::AnswerPrompt { .. } => "AnswerPrompt([HOST-ONLY])",
            Self::Disconnect { .. } => "Disconnect",
            Self::AdoptReady { .. } => "AdoptReady",
        };
        write!(f, "SshRequest::{name}")
    }
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Save(input) => {
                input.clone().into_profile().validate()?;
            }
            Self::Connect { cols, rows, .. } => {
                shellfox_ssh::pty::RemotePty::new(*cols, *rows)?;
            }
            _ => {}
        }
        Ok(())
    }
}
impl Input {
    pub(crate) fn into_profile(self) -> shellfox_ssh::ProfileInput {
        shellfox_ssh::ProfileInput {
            id: self.id,
            name: self.name,
            host: self.host,
            port: self.port,
            user: self.user,
            password: match self.password {
                Password::Keep => shellfox_ssh::PasswordUpdate::Keep,
                Password::Clear => shellfox_ssh::PasswordUpdate::Clear,
                Password::Set(v) => shellfox_ssh::PasswordUpdate::Set(v),
            },
            key_file: self.key_file,
            remote_cwd: self.remote_cwd,
        }
    }
}
