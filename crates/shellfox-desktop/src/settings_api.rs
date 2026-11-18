//! Presentation contract for host-provided settings capabilities. No OS or network work lives here.
use std::{future::Future, pin::Pin};

pub type Reply<'a> = Pin<Box<dyn Future<Output = anyhow::Result<Response>> + Send + 'a>>;
pub trait SettingsServices: Send + Sync {
    fn request(&self, request: Request) -> Reply<'_>;
}
#[derive(Clone, Debug)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub elevation: bool,
}
#[derive(Clone, Debug)]
pub struct Catalog {
    pub profiles: Vec<Profile>,
    pub default_profile_id: Option<String>,
    pub platform: String,
    pub python_detected: Option<String>,
    pub python_reason: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorKind {
    Accent,
    Background,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomColors {
    pub accent: Vec<String>,
    pub background: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Disconnected,
    Connecting,
    Reconnecting,
    Connected,
    AwaitingApproval,
    Rejected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkStatus {
    Disabled,
    Starting,
    LoginRequired,
    Online,
    Error,
}
#[derive(Clone, Debug)]
pub struct Peer {
    pub id: String,
    pub hostname: String,
    pub addresses: Vec<String>,
    pub remembered: bool,
    pub host_confirmed: bool,
    pub online: bool,
    pub connection: Connection,
    pub last_error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Approval {
    pub peer_id: String,
    pub hostname: String,
}
#[derive(Clone, Debug)]
pub struct PendingClient {
    pub id: String,
    pub hostname: String,
    pub addresses: Option<Vec<String>>,
}
#[derive(Clone, Debug)]
pub struct RemoteAttempt {
    pub id: String,
    pub kind: RemoteAttemptKind,
}
#[derive(Clone, Debug)]
pub enum RemoteAttemptKind {
    Resume,
    Enable,
    Login,
    Connect { peer_id: String },
}
#[derive(Clone, Debug)]
pub struct RemoteState {
    pub enabled: bool,
    pub hostname: String,
    pub status: NetworkStatus,
    pub login_url: Option<String>,
    pub error: Option<String>,
    pub peers: Vec<Peer>,
    pub pending: Vec<PendingClient>,
    pub approvals: Vec<Approval>,
    pub attempts: Vec<RemoteAttempt>,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct SshProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub has_password: bool,
    pub key_file: Option<String>,
    pub remote_cwd: Option<String>,
}
/// Deliberately not Debug. Empty password means preserve; Clear is explicit.
pub enum Password {
    Preserve,
    Clear,
    Replace(String),
}
pub struct SshInput {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Password,
    pub key_file: Option<String>,
    pub remote_cwd: Option<String>,
}
pub enum Request {
    RemoteState,
    ConfigureRemote { enabled: bool, hostname: String },
    Login { auth_key: Option<String> },
    Connect(String),
    CancelAttempt(String),
    ContinueAttempt(String),
    Disconnect(String),
    Approve(String),
    Reject(String),
    Revoke(String),
    Forget(String),
    SshList,
    SshSave(SshInput),
    SshDelete(String),
    SshImport,
    ValidateSshKey(shellfox_service::ssh::SelectedKey),
}
pub enum Response {
    Remote(RemoteState),
    SshProfiles(Vec<SshProfile>),
    SshImported {
        profiles: Vec<SshProfile>,
        found: usize,
        added: usize,
        updated: usize,
    },
    KeyFile(Option<String>),
}
