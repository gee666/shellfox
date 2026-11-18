//! Explicit Rust-only manager recovery. No C ABI profile or default engine changes.
use shellfox_client::manager_v2::{
    CursorError, ManagerBindingWitness, ManagerClient, ManagerCursor,
};
use shellfox_ghostty_vt::extension::{SnapshotLimits, VerifiedNativeExtension};
use shellfox_protocol::{
    manager_v2 as m, AuthenticatedPeerId, CancellationToken, OutboundTransport,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
#[derive(Debug)]
pub enum ManagerRecoveryError {
    Client(shellfox_client::manager_v2::ClientError),
    Cursor(CursorError),
    Engine(crate::Status),
}
impl From<crate::Status> for ManagerRecoveryError {
    fn from(value: crate::Status) -> Self {
        Self::Engine(value)
    }
}
impl From<shellfox_client::manager_v2::ClientError> for ManagerRecoveryError {
    fn from(value: shellfox_client::manager_v2::ClientError) -> Self {
        Self::Client(value)
    }
}
pub(crate) struct ViewState {
    pub generation: u64,
    pub retired: bool,
    pub cursor: Option<ManagerCursor>,
    pub installed: Option<(u64, m::Id)>,
    pub data: VecDeque<(u64, u64)>,
    pub failure: Option<crate::Status>,
    pub pending_ack: Option<m::AcknowledgeTerminal>,
    pub origin: Option<RecoveryOrigin>,
}
#[derive(Clone)]
pub(crate) struct RecoveryOrigin {
    pub witness: ManagerBindingWitness,
    pub generation: u64,
    pub cancel: CancellationToken,
}
impl RecoveryOrigin {
    pub fn check(&self, state: &ViewState) -> Result<(), crate::Status> {
        if self.cancel.is_cancelled() || state.retired || state.generation != self.generation {
            Err(crate::Status::Stale)
        } else {
            Ok(())
        }
    }
}
pub(crate) struct RecoveryWork {
    pub cursor: ManagerCursor,
    pub receiver: VerifiedNativeExtension,
    pub limits: SnapshotLimits,
    pub origin: RecoveryOrigin,
    pub state: Arc<Mutex<ViewState>>,
    pub shared: Arc<crate::engine::Shared>,
    pub charge: usize,
}
/// Owns a fresh decoded candidate, never the current live target. Cannot be
/// constructed by a caller or moved to another native engine owner thread.
pub struct PreparedNative0Install {
    pub(crate) terminal: shellfox_ghostty_vt::Terminal,
    pub(crate) work: RecoveryWork,
    pub(crate) ticket: u64,
}
/// Caller closes on the native owner thread. Transport/policy are the existing
/// manager client, never a new approval issuer or test authority endpoint.
#[must_use = "close the native view on its owner thread"]
pub struct ManagerRecovery<T: OutboundTransport> {
    client: Arc<ManagerClient<T>>,
    terminal: u64,
    state: Arc<Mutex<ViewState>>,
    cancel: CancellationToken,
    receiver: VerifiedNativeExtension,
    limits: SnapshotLimits,
    ack_slot: tokio::sync::Semaphore,
}
impl<T: OutboundTransport> ManagerRecovery<T> {
    pub fn native0(
        peer: AuthenticatedPeerId,
        client: m::Id,
        transport: T,
        cols: u16,
        rows: u16,
        limits: SnapshotLimits,
    ) -> Result<Self, ManagerRecoveryError> {
        let receiver = VerifiedNativeExtension::verify().map_err(|_| crate::Status::Engine)?;
        let identity = String::from_utf8(receiver.identity().as_bytes().to_vec())
            .map_err(|_| crate::Status::Invalid)?;
        let codec = m::Native0Codec::new(
            m::EngineIdentity::try_from(identity).map_err(|_| crate::Status::Invalid)?,
        );
        let client = Arc::new(ManagerClient::native0(peer, client, transport, codec)?);
        let terminal = crate::engine::create(cols, rows)?;
        Self::bind_native0(client, terminal, receiver, limits)
    }
    // One connection may own several UI-native views. No additional PAIR or
    // parser is created by binding; import still constructs the fresh target.
    pub fn bind_native0(
        client: Arc<ManagerClient<T>>,
        terminal: u64,
        receiver: VerifiedNativeExtension,
        limits: SnapshotLimits,
    ) -> Result<Self, ManagerRecoveryError> {
        let shared = crate::engine::lookup(terminal)?;
        if shared.owner != std::thread::current().id() {
            return Err(crate::Status::WrongThread.into());
        }
        let queue = shared.queue.lock().map_err(|_| crate::Status::Panic)?;
        if queue.closed
            || queue.failed
            || !queue.chunks.is_empty()
            || queue.accepted != queue.consumed
        {
            return Err(crate::Status::Busy.into());
        }
        // This existing role bit is also the engine-lifetime exclusive claim.
        // Never release it on view Drop/retirement: retained work can outlive
        // that handle. A new association requires a fresh native engine.
        shared
            .passive_manager
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .map_err(|_| crate::Status::Busy)?;
        drop(queue);
        Ok(Self {
            client,
            terminal,
            state: Arc::new(Mutex::new(ViewState {
                generation: 0,
                retired: false,
                cursor: None,
                installed: None,
                data: VecDeque::new(),
                failure: None,
                pending_ack: None,
                origin: None,
            })),
            cancel: CancellationToken::new(),
            receiver,
            limits,
            ack_slot: tokio::sync::Semaphore::new(1),
        })
    }
    fn ensure_live_engine(&self) -> Result<(), ManagerRecoveryError> {
        let shared = crate::engine::lookup(self.terminal)?;
        let queue = shared.queue.lock().map_err(|_| crate::Status::Panic)?;
        if queue.closed {
            return Err(crate::Status::Stale.into());
        }
        if queue.failed {
            return Err(crate::Status::Engine.into());
        }
        Ok(())
    }
    pub fn client(&self) -> &ManagerClient<T> {
        &self.client
    }
    pub fn terminal_handle(&self) -> u64 {
        self.terminal
    }
    pub async fn pair(
        &self,
        attempt: m::Id,
    ) -> Result<m::Reply<m::PairSelection>, ManagerRecoveryError> {
        Ok(self.client.pair(attempt).await?)
    }
    pub async fn discover(
        &self,
        policy: shellfox_client::manager_v2::FallbackPolicy,
    ) -> Result<shellfox_client::manager_v2::DiscoveryOutcome, ManagerRecoveryError> {
        Ok(self.client.discover(policy).await?)
    }
    pub fn prepare_pending(&mut self) -> Result<PreparedNative0Install, ManagerRecoveryError> {
        Ok(crate::engine::with_engine(
            self.terminal,
            crate::engine::prepare_native0_pending,
        )?)
    }
    pub fn commit_prepared(
        &mut self,
        prepared: PreparedNative0Install,
    ) -> Result<(), ManagerRecoveryError> {
        Ok(crate::engine::with_engine(self.terminal, |owned| {
            crate::engine::commit_native0(owned, prepared)
        })?)
    }
    pub fn pump(&mut self) -> Result<(), ManagerRecoveryError> {
        crate::engine::with_engine(self.terminal, crate::engine::pump)?;
        if let Some(error) = self.state.lock().map_err(|_| crate::Status::Panic)?.failure {
            return Err(error.into());
        }
        Ok(())
    }
    pub fn cancel_pending(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        if let Ok(mut state) = self.state.lock() {
            state.generation = state.generation.saturating_add(1);
            state.origin = None;
            state.pending_ack = None;
            state.installed = None;
            state.data.clear();
        }
        if let Ok(shared) = crate::engine::lookup(self.terminal) {
            if let Ok(mut queue) = shared.queue.lock() {
                queue.discard_pending();
            }
        }
    }
    pub fn reconnect(&mut self, client: m::Id) {
        self.cancel_pending();
        self.client.reconnect(client);
    }
    pub fn close(&mut self) -> Result<(), ManagerRecoveryError> {
        self.cancel_pending();
        self.state.lock().map_err(|_| crate::Status::Panic)?.retired = true;
        Ok(crate::engine::close(self.terminal)?)
    }
}
impl<T: OutboundTransport> Drop for ManagerRecovery<T> {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Ok(mut state) = self.state.lock() {
            state.retired = true;
            state.generation = state.generation.saturating_add(1);
            state.origin = None;
            state.pending_ack = None;
            state.installed = None;
        }
    }
}
#[path = "client_manager_abi.rs"]
pub(super) mod abi;
#[path = "client_manager_attachment.rs"]
mod attachment;
#[path = "client_manager_bindings.rs"]
pub(in crate::client) mod bindings;
#[path = "client_manager_public.rs"]
mod public_api;
pub use public_api::{ManagerOwnerView, PendingManagerInput, PendingManagerResize};
#[path = "client_manager_poll.rs"]
mod global_poll;
pub use global_poll::ManagerPollContext;
#[path = "client_manager_consumption.rs"]
mod consumption;
#[path = "client_manager_events.rs"]
pub(in crate::client) mod events;
#[path = "client_manager_input.rs"]
mod input;
#[path = "client_manager_native_input.rs"]
mod native_input;
#[path = "client_manager_operations.rs"]
pub(in crate::client) mod operations;
#[cfg(test)]
#[path = "client_manager_recovery_tests.rs"]
mod tests;
