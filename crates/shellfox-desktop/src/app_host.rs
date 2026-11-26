//! The application's host connection and bounded window clients. Keep a clone
//! in a GPUI Global, not just in Roots. Dropping a window never stops the host.
pub mod adapter;
mod backend;
mod close;
mod policy;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, ensure};
pub use backend::{AppHostBackend, AttachedBackend, HostFuture};
pub use policy::*;
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

pub const MAX_WINDOWS: usize = 64;
pub struct WindowClient<C> {
    pub id: WindowId,
    pub client: C,
}
struct RegisteredWindow<C> {
    client: C,
    guard: WindowCloseGuard,
}
struct PendingClose {
    ticket: CloseTicket,
    windows: Vec<WindowId>,
    detached: HashSet<WindowId>,
    stop_host: bool,
    finishing: bool,
}
struct State<C> {
    windows: BTreeMap<WindowId, RegisteredWindow<C>>,
    next_window: u64,
    revision: u64,
    pending: Option<PendingClose>,
    cleanup_retained: bool,
    stopped: bool,
}
struct Shared<B: AppHostBackend> {
    backend: B,
    identity: Arc<()>,
    state: Mutex<State<B::Client>>,
    operation: tokio::sync::Mutex<()>,
}
pub struct AppHost<B: AppHostBackend = AttachedBackend> {
    shared: Arc<Shared<B>>,
}
impl<B: AppHostBackend> Clone for AppHost<B> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}
impl AppHost {
    pub(crate) fn attached(
        config: crate::host_client::HostEndpointConfig,
        route: crate::host_process::Route,
        owned: bool,
    ) -> Result<Self> {
        let runtime = crate::host_client::attached::AttachedRuntime::new()?;
        let owned = owned.into();
        Ok(Self::with_backend(AttachedBackend {
            config,
            route,
            runtime,
            owned,
        }))
    }
    /// Attached to a host this desktop did not start, such as `host run`.
    pub(crate) fn is_external(&self) -> bool {
        !self.shared.backend.owned()
    }
    /// Settled once a host started in the background answers.
    pub(crate) fn set_owned(&self, owned: bool) {
        self.shared
            .backend
            .owned
            .store(owned, std::sync::atomic::Ordering::SeqCst);
    }
    pub(crate) async fn route(&self) -> Result<crate::host_process::HostRoute> {
        self.shared.backend.route.get().await
    }

    pub(crate) async fn chrome_client(&self) -> Result<crate::host_client::HostClient> {
        self.shared.backend.fork_window().await
    }
}
impl<B: AppHostBackend> AppHost<B> {
    pub fn with_backend(backend: B) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                identity: Arc::new(()),
                state: Mutex::new(State {
                    windows: BTreeMap::new(),
                    next_window: 0,
                    revision: 0,
                    pending: None,
                    cleanup_retained: false,
                    stopped: false,
                }),
                operation: tokio::sync::Mutex::new(()),
            }),
        }
    }
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
    fn state(&self) -> Result<MutexGuard<'_, State<B::Client>>> {
        self.shared
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("App host registry poisoned; ownership retained."))
    }
    /// Whether the host is stopped once any in-flight close has finished.
    pub async fn stopped_after_operations(&self) -> Result<bool> {
        let _operation = self.shared.operation.lock().await;
        self.is_stopped()
    }
    /// Use this actual owner count for request_close, never persisted tab count.
    pub async fn owned_terminal_count(&self) -> Result<usize> {
        self.shared.backend.owned_terminal_count().await
    }
    /// Even the first Root receives a fork. Clones only share that same window.
    pub async fn open_window(&self) -> Result<WindowClient<B::Client>> {
        let _operation = self.shared.operation.lock().await;
        {
            let state = self.state()?;
            ensure!(
                !state.stopped && state.pending.is_none() && !state.cleanup_retained,
                "Host is closing or cleanup is unconfirmed; no new window was admitted."
            );
            ensure!(state.windows.len() < MAX_WINDOWS, "Window limit reached.");
        }
        let client = self.shared.backend.fork_window().await?;
        let mut state = self.state()?;
        // A confirmation prepared while startup awaited must be invalidated.
        state.next_window = state
            .next_window
            .checked_add(1)
            .context("Window identity exhausted.")?;
        let id = WindowId(state.next_window);
        state.windows.insert(
            id,
            RegisteredWindow {
                client: client.clone(),
                guard: WindowCloseGuard::Ready,
            },
        );
        state.revision += 1;
        Ok(WindowClient { id, client })
    }
    /// Call on every dirty/pending change, including hidden file panels. During
    /// finish_close the adapter must disable edits and host mutations in targets.
    pub fn set_guard(&self, id: WindowId, guard: WindowCloseGuard) -> Result<()> {
        let mut state = self.state()?;
        ensure!(
            !state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.windows.contains(&id)),
            "This window is already closing."
        );
        let window = state
            .windows
            .get_mut(&id)
            .context("Window is not registered.")?;
        if window.guard != guard {
            window.guard = guard;
            state.revision += 1;
        }
        Ok(())
    }
    pub fn window_count(&self) -> Result<usize> {
        Ok(self.state()?.windows.len())
    }
    /// True only after final finish_close confirmed backend shutdown. Empty OS
    /// windows/registrations, startup errors and failed cleanup are not Stop.
    /// An error must keep the application alive, never authorize cx.quit().
    pub fn is_stopped(&self) -> Result<bool> {
        Ok(self.state()?.stopped)
    }
    /// The host process ended on its own; there is nothing left to stop.
    pub(crate) fn host_ended(&self) {
        crate::launch_lock::host_ended();
        if let Ok(mut state) = self.state() {
            state.stopped = true;
            state.revision += 1;
        }
    }
    /// Stops an idle desktop-started host when the UI ends without a confirmed
    /// close, such as after a graphics failure. Windows must already be gone.
    pub async fn cleanup(&self) -> Result<()> {
        let _operation = self.shared.operation.lock().await;
        {
            let mut state = self.state()?;
            ensure!(
                state.windows.is_empty() && state.pending.is_none(),
                "Cleanup cannot remove registered or pending windows"
            );
            if state.stopped {
                return Ok(());
            }
            // Also fences a cancelled wait; a later call resumes the same Stop.
            state.cleanup_retained = true;
            state.revision += 1;
        }
        self.shared.backend.stop().await?;
        let mut state = self.state()?;
        state.stopped = true;
        state.cleanup_retained = false;
        state.revision += 1;
        Ok(())
    }
    /// The OS is ending this UI (see system_end.rs): stop the host with its
    /// shells, as an approved Quit would, without the windows that can no
    /// longer close themselves.
    pub async fn stop_for_system_end(&self) -> Result<()> {
        {
            let mut state = self.state()?;
            if state.stopped {
                return Ok(());
            }
            state.windows.clear();
            state.pending = None;
            state.cleanup_retained = true;
            state.revision += 1;
        }
        self.shared.backend.stop().await?;
        let mut state = self.state()?;
        state.stopped = true;
        state.cleanup_retained = false;
        state.revision += 1;
        Ok(())
    }
    /// Check again on the UI thread before native construction after an async
    /// open. An app quit may have closed this registration while UI work waited.
    pub fn is_window_available(&self, id: WindowId) -> Result<bool> {
        let state = self.state()?;
        Ok(state.windows.contains_key(&id)
            && !state.stopped
            && !state.cleanup_retained
            && !state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.windows.contains(&id)))
    }
    #[cfg(test)]
    pub fn is_closing(&self, id: WindowId) -> Result<bool> {
        Ok(self
            .state()?
            .pending
            .as_ref()
            .is_some_and(|pending| pending.windows.contains(&id)))
    }
    pub fn cleanup_retained(&self) -> Result<bool> {
        Ok(self.state()?.cleanup_retained)
    }
    /// `owned_terminal_count` is the service's actual owned count, not metadata
    /// tab count. As in TerminalQuitGuard, an idle app needs no quit confirmation.
    pub fn request_close(
        &self,
        target: CloseTarget,
        owned_terminal_count: usize,
    ) -> Result<CloseDecision> {
        let state = self.state()?;
        ensure!(!state.stopped, "Host is already stopped.");
        if let Some(pending) = &state.pending {
            ensure!(
                pending.ticket.target == target,
                "Another close operation is pending."
            );
            return Ok(CloseDecision::Ready(pending.ticket.clone()));
        }
        let windows = Self::targets(&state, target)?;
        let blocked: Vec<_> = windows
            .iter()
            .filter_map(|id| {
                let guard = state.windows[id].guard;
                (guard != WindowCloseGuard::Ready).then_some((*id, guard))
            })
            .collect();
        if !blocked.is_empty() {
            return Ok(CloseDecision::Blocked(blocked));
        }
        let ticket = CloseTicket {
            owner: Arc::downgrade(&self.shared.identity),
            revision: state.revision,
            target,
        };
        if windows.len() == state.windows.len() && owned_terminal_count != 0 {
            Ok(CloseDecision::Confirm(CloseConfirmation {
                ticket,
                terminal_count: owned_terminal_count,
            }))
        } else {
            Ok(CloseDecision::Ready(ticket))
        }
    }
    fn targets(state: &State<B::Client>, target: CloseTarget) -> Result<Vec<WindowId>> {
        match target {
            CloseTarget::Quit => Ok(state.windows.keys().copied().collect()),
            CloseTarget::Window(id) => {
                ensure!(state.windows.contains_key(&id), "Window is not registered.");
                Ok(vec![id])
            }
        }
    }
}
