//! Closed, authenticated LOCAL operator IPC. Never a network Manager method.
use super::{RemoteControlState, RemoteRequest};
use shellfox_protocol::{AppError, AuthenticatedPeerId, Validate, ValidationError};
use std::{collections::HashSet, net::IpAddr};
pub(crate) const METHOD: &str = "private.remote-control.v1";
/// Display metadata only; this is not an authenticated incoming identity.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteOperatorPending {
    pub id: String,
    pub hostname: String,
    pub clients: usize,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteOperatorState {
    pub enabled: bool,
    pub hostname: String,
    pub status: super::native::RemoteNetworkStatus,
    pub login_url: Option<String>,
    pub error: Option<AppError>,
    pub peers: Vec<super::native::RemotePeer>,
    pub pending: Vec<RemoteOperatorPending>,
    pub approvals: Vec<shellfox_remote::Approval>,
    pub attempts: Vec<super::native::RemoteAttempt>,
    pub cleanup_retained: bool,
    pub incoming_legacy_terminal_attachment: bool,
    pub terminal_capability_reason: String,
    #[serde(default)]
    pub root_account: bool,
}
impl std::fmt::Debug for RemoteOperatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteOperatorState")
            .field("enabled", &self.enabled)
            .field("status", &self.status)
            .field("peers", &self.peers.len())
            .finish_non_exhaustive()
    }
}
impl From<&RemoteControlState> for RemoteOperatorState {
    fn from(state: &RemoteControlState) -> Self {
        Self {
            enabled: state.enabled,
            hostname: state.hostname.clone(),
            status: state.status,
            login_url: state.login_url.clone(),
            error: state.error.clone(),
            peers: state.peers.clone(),
            pending: state
                .pending
                .iter()
                .map(|p| RemoteOperatorPending {
                    id: p.peer.id.clone(),
                    hostname: p.peer.hostname.clone(),
                    clients: p.clients,
                })
                .collect(),
            approvals: state.approvals.clone(),
            attempts: state.attempts.clone(),
            cleanup_retained: state.cleanup_retained,
            incoming_legacy_terminal_attachment: state.incoming_legacy_terminal_attachment,
            terminal_capability_reason: state.terminal_capability_reason.clone(),
            root_account: state.root_account,
        }
    }
}
impl From<RemoteOperatorState> for RemoteControlState {
    fn from(state: RemoteOperatorState) -> Self {
        Self {
            enabled: state.enabled,
            hostname: state.hostname,
            status: state.status,
            login_url: state.login_url,
            error: state.error,
            peers: state.peers,
            pending: state
                .pending
                .into_iter()
                .map(|peer| shellfox_remote::PendingPeer {
                    peer: shellfox_transport::incoming::PeerIdentity {
                        id: peer.id,
                        hostname: peer.hostname,
                    },
                    clients: peer.clients,
                })
                .collect(),
            approvals: state.approvals,
            attempts: state.attempts,
            cleanup_retained: state.cleanup_retained,
            incoming_legacy_terminal_attachment: state.incoming_legacy_terminal_attachment,
            terminal_capability_reason: state.terminal_capability_reason,
            root_account: state.root_account,
        }
    }
}
pub(crate) type Reply = Result<RemoteOperatorState, AppError>;
impl Validate for RemoteRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        RemoteRequest::validate(self).map_err(|_| ValidationError)
    }
}
fn valid_text(value: &str, maximum: usize) -> bool {
    value.len() <= maximum && !value.chars().any(char::is_control)
}
fn valid_error(error: &AppError) -> bool {
    !error.message.is_empty() && valid_text(&error.message, 2048)
}
fn peer(id: &str, hostname: &str) -> bool {
    AuthenticatedPeerId::new(id).is_ok() && !hostname.is_empty() && valid_text(hostname, 253)
}
pub(crate) fn validate_state(state: &RemoteOperatorState) -> Result<(), ValidationError> {
    let fail = || ValidationError;
    RemoteRequest::Configure {
        enabled: false,
        hostname: state.hostname.clone(),
    }
    .validate()
    .map_err(|_| fail())?;
    if state.peers.len() > 1000
        || state.pending.len() > 1000
        || state.approvals.len() > 1000
        || state.attempts.len() > 1000
        || !valid_text(&state.terminal_capability_reason, 2048)
        || state.error.as_ref().is_some_and(|e| !valid_error(e))
        || state.login_url.as_ref().is_some_and(|url| {
            url.is_empty() || !valid_text(url, 2048) || url.chars().any(char::is_whitespace)
        })
    {
        return Err(fail());
    }
    let mut ids = HashSet::new();
    for p in &state.peers {
        if !ids.insert(&p.id)
            || !peer(&p.id, &p.hostname)
            || p.addresses.len() > 16
            || (p.online && p.addresses.is_empty())
            || p.addresses
                .iter()
                .any(|a| a.len() > 64 || a.parse::<IpAddr>().is_err())
            || p.attempt_id.is_some_and(|id| id.is_nil())
            || p.last_error.as_ref().is_some_and(|e| !valid_error(e))
        {
            return Err(fail());
        }
    }
    let mut ids = HashSet::new();
    for p in &state.pending {
        if !ids.insert(&p.id) || !peer(&p.id, &p.hostname) || p.clients > 10000 {
            return Err(fail());
        }
    }
    let mut ids = HashSet::new();
    for approval in &state.approvals {
        if !ids.insert(&approval.peer_id)
            || !peer(&approval.peer_id, &approval.hostname)
            || approval.permission != shellfox_remote::ViewPermission::Write
            || !approval.file_roots.is_empty()
        {
            return Err(fail());
        }
    }
    let mut ids = HashSet::new();
    for a in &state.attempts {
        if a.id.is_nil() || !ids.insert(a.id) {
            return Err(fail());
        }
        if let super::native::AttemptKind::Connect { peer_id } = &a.kind {
            AuthenticatedPeerId::new(peer_id).map_err(|_| fail())?;
        }
    }
    Ok(())
}
