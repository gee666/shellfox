#[path = "approval_management.rs"]
mod management;
#[cfg(test)]
#[path = "approval_policy_tests.rs"]
mod policy_tests;
#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
use crate::{error, policy, Approval, PollWake, PrivateApprovalStore, ViewPermission};
use shellfox_files::FileService;
use shellfox_protocol::{AppError, ClientId, ErrorCode, RemoteCommand};
use shellfox_transport::incoming::PeerIdentity;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

const ATTEMPT_TTL: u64 = 3_600_000;
const DENIED_TTL: u64 = 300_000;
#[derive(Clone, Debug)]
pub struct PendingPeer {
    pub peer: PeerIdentity,
    pub clients: usize,
}
#[derive(Clone, Debug)]
pub struct ApprovalState {
    pub approvals: Vec<Approval>,
    pub pending: Vec<PendingPeer>,
}
struct Grant {
    approval: Approval,
    epoch: u64,
    files: Vec<Arc<FileService>>,
}
#[derive(Default)]
struct Attempts {
    seen: HashMap<ClientId, u64>,
    denied: HashMap<ClientId, u64>,
}
struct State {
    grants: HashMap<String, Grant>,
    pending: HashMap<String, PeerIdentity>,
    attempts: HashMap<String, Attempts>,
    epoch: u64,
    enabled: bool,
    native_unavailable: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            grants: HashMap::new(),
            pending: HashMap::new(),
            attempts: HashMap::new(),
            epoch: 0,
            enabled: true,
            native_unavailable: false,
        }
    }
}
#[derive(Clone)]
pub struct ApprovalControl {
    inner: Arc<Mutex<State>>,
    changed: tokio::sync::watch::Sender<u64>,
    pub(super) management: Arc<tokio::sync::Mutex<()>>,
}
impl Default for ApprovalControl {
    fn default() -> Self {
        Self {
            inner: Arc::default(),
            changed: tokio::sync::watch::channel(0).0,
            management: Arc::default(),
        }
    }
}

/// An unforgeable host-local authorization ticket, checked again on the owner.
#[derive(Clone)]
pub struct Actor {
    pub peer_id: String,
    pub client_id: ClientId,
    pub permission: ViewPermission,
    pub approval_epoch: u64,
    pub(crate) view_epoch: u64,
    policy: ApprovalControl,
    files: Vec<Arc<FileService>>,
    pub(crate) wake: PollWake,
}
impl Actor {
    pub fn view_epoch(&self) -> u64 {
        self.view_epoch
    }
    /// Per-view wake for the pending event poll. A new pairing gets a new one.
    pub fn poll_wake(&self) -> PollWake {
        self.wake.clone()
    }
    /// Serializes revocation with the entire synchronous owner operation.
    /// Never await or call this recursively inside `operation`.
    pub fn with_authorized<T>(
        &self,
        command: Option<&RemoteCommand>,
        operation: impl FnOnce() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let state = self.policy.lock()?;
        self.check(&state)?;
        if let Some(command) = command {
            policy::authorize(self.permission, command)?;
        }
        operation()
    }
    fn check(&self, state: &State) -> Result<(), AppError> {
        if !state.enabled || state.native_unavailable {
            return Err(denied());
        }
        let grant = state.grants.get(&self.peer_id).ok_or_else(denied)?;
        if grant.epoch != self.approval_epoch
            || grant.approval.permission != self.permission
            || state
                .attempts
                .get(&self.peer_id)
                .is_some_and(|a| a.denied.contains_key(&self.client_id))
        {
            return Err(denied());
        }
        Ok(())
    }
    /// Explicit revocable ApprovedFullAccount authority for approved devices.
    /// This is not inferred from legacy file_roots or an empty vector. Use only
    /// while with_authorized holds the current grant/epoch commit fence.
    pub fn file_capabilities(&self) -> &[Arc<FileService>] {
        &self.files
    }
}
fn denied() -> AppError {
    error(
        ErrorCode::AccessDenied,
        "The host declined, cancelled or revoked this connection. Start a new attempt.",
        false,
    )
}
impl ApprovalControl {
    /// Local enable switch. Disable fences tickets without deleting approvals.
    pub fn set_enabled(&self, enabled: bool) -> Result<(), AppError> {
        let mut state = self.lock()?;
        if enabled && state.native_unavailable {
            return Err(error(
                ErrorCode::Unsupported,
                "Native approval authority remains unavailable; enable cannot restore it.",
                false,
            ));
        }
        if state.enabled && !enabled {
            state.enabled = false;
            let mut epoch = state.epoch;
            for grant in state.grants.values_mut() {
                epoch = epoch.checked_add(1).ok_or_else(denied)?;
                grant.epoch = epoch;
            }
            state.epoch = epoch;
        } else {
            state.enabled = enabled;
        }
        self.notify();
        Ok(())
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changed.subscribe()
    }
    fn notify(&self) {
        self.changed
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
    pub(crate) fn expire_pending(&self, now: u64) {
        if let Ok(mut state) = self.lock() {
            let before = state.pending.len();
            Self::expire(&mut state, now);
            if before != state.pending.len() {
                self.notify();
            }
        }
    }
    fn lock(&self) -> Result<MutexGuard<'_, State>, AppError> {
        self.inner
            .lock()
            .map_err(|_| error(ErrorCode::Internal, "Approval state unavailable.", false))
    }
    pub fn state(&self) -> Result<ApprovalState, AppError> {
        let state = self.lock()?;
        Ok(ApprovalState {
            approvals: state.grants.values().map(|g| g.approval.clone()).collect(),
            pending: state
                .pending
                .values()
                .map(|peer| PendingPeer {
                    peer: peer.clone(),
                    clients: state.attempts.get(&peer.id).map_or(0, |a| a.seen.len()),
                })
                .collect(),
        })
    }
    pub fn authorize(
        &self,
        peer: &PeerIdentity,
        client: ClientId,
        now: u64,
    ) -> Result<Actor, AppError> {
        let mut state = self.lock()?;
        if !state.enabled || state.native_unavailable {
            return Err(denied());
        }
        Self::expire(&mut state, now);
        if !state.attempts.contains_key(&peer.id) && state.attempts.len() >= 1000 {
            return Err(error(ErrorCode::Validation, "Too many remote peers.", true));
        }
        let approved = state.grants.contains_key(&peer.id);
        if !approved && !state.pending.contains_key(&peer.id) && state.pending.len() >= 100 {
            return Err(error(
                ErrorCode::Validation,
                "Too many pending access requests.",
                true,
            ));
        }
        let attempts = state.attempts.entry(peer.id.clone()).or_default();
        if attempts.denied.contains_key(&client) {
            return Err(denied());
        }
        if !attempts.seen.contains_key(&client)
            && attempts.seen.len() + attempts.denied.len() >= 256
        {
            return Err(error(
                ErrorCode::Validation,
                "Too many access attempts.",
                true,
            ));
        }
        let new_attempt = attempts.seen.insert(client, now).is_none();
        if let Some(grant) = state.grants.get(&peer.id) {
            return Ok(Actor {
                peer_id: peer.id.clone(),
                client_id: client,
                permission: grant.approval.permission,
                approval_epoch: grant.epoch,
                view_epoch: 0,
                policy: self.clone(),
                files: grant.files.clone(),
                wake: PollWake::default(),
            });
        }
        state
            .pending
            .entry(peer.id.clone())
            .or_insert_with(|| peer.clone());
        if new_attempt {
            self.notify();
        }
        Err(error(
            ErrorCode::ApprovalRequired,
            "Waiting for approval in Shellfox on the host.",
            true,
        ))
    }
    pub fn withdraw(
        &self,
        peer: &PeerIdentity,
        client: ClientId,
        now: u64,
    ) -> Result<(), AppError> {
        let mut state = self.lock()?;
        Self::expire(&mut state, now);
        if !state.attempts.contains_key(&peer.id) && state.attempts.len() >= 1000 {
            return Err(denied());
        }
        let attempts = state.attempts.entry(peer.id.clone()).or_default();
        if !attempts.denied.contains_key(&client) && attempts.denied.len() >= 256 {
            return Err(denied());
        }
        attempts.seen.remove(&client);
        attempts.denied.insert(client, now + DENIED_TTL);
        if attempts.seen.is_empty() {
            state.pending.remove(&peer.id);
        }
        self.notify();
        Ok(())
    }
    fn expire(state: &mut State, now: u64) {
        state.attempts.retain(|_, a| {
            a.seen.retain(|_, t| now.saturating_sub(*t) <= ATTEMPT_TTL);
            a.denied.retain(|_, t| *t > now);
            !a.seen.is_empty() || !a.denied.is_empty()
        });
        let pending: HashSet<_> = state
            .attempts
            .iter()
            .filter(|(_, a)| !a.seen.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        state.pending.retain(|id, _| pending.contains(id));
    }
}
