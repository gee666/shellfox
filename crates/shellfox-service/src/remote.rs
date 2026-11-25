//! Remote dispatch borrows the existing service; it never opens a second Host.
#[cfg(test)]
mod legacy_scope_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod v1_loopback_tests;
#[cfg(test)]
mod wake_tests;
use crate::Service;
use shellfox_core::{SessionId, Snapshot};
use shellfox_protocol::{self as wire, AppError, ErrorCode, RemoteCommand, Validate};
use shellfox_remote::{
    adapter::{self, ManagerFacts},
    capability_unavailable, Actor, OwnerCommand, ServiceHost,
};
use std::collections::HashMap;

mod close;
mod commands;
mod creation_profile;
mod dispatcher;
mod files;
mod privacy;
mod stats;
mod v1_activity;
mod v1_delivery;
mod v1_launch;
mod v1_privilege;
#[cfg(unix)]
pub(crate) use v1_privilege::host_account;
mod v1_replay;
mod v1_terminal;
#[cfg(windows)]
mod wsl_files;
pub use dispatcher::{remote_channel, RemoteDispatcher, RemoteInbox};
pub mod facts;
mod source;
pub use facts::{
    ManagerFactsFuture, ManagerFactsProvider, ManagerFactsRequest, ManagerFactsResult,
    RuntimeFactIdentity,
};
pub use source::{SourceFactsInstaller, SourceFactsProjector, SourceFactsProviderSlot};
mod host;
pub use host::RemoteBackend;
mod setup;
pub use setup::{RemoteBootResources, RemoteHostSetup, RemoteResources};
pub(crate) mod outgoing;
pub use outgoing::{PeerFuture, PeerResponse};
pub(crate) mod operator;
pub(crate) mod outgoing_wire;
pub use operator::{RemoteOperatorPending, RemoteOperatorState};
pub mod native;
pub mod preferences;
pub mod runtime;
pub use native::{AuthKey, RemoteControl, RemoteControlState, RemoteRequest, RemoteResponse};
pub use runtime::{GoNetworkFactory, RemoteNetwork, RemoteNetworkFactory, RemoteRuntime};
/// Bounded loopback-only boot setting, never a peer request URL.
pub use shellfox_transport::LoopbackOrigin as ControlServerOrigin;
struct View {
    epoch: u64,
    revision: u64,
    visible: Vec<SessionId>,
    controller: u64,
    terminals: HashMap<shellfox_core::TabId, v1_delivery::Delivery>,
    replays: HashMap<shellfox_core::TabId, v1_replay::Replay>,
    wake: shellfox_remote::PollWake,
    activity: v1_activity::Reported,
}
impl View {
    /// True when the next Poll would return events, given the host's latest revision.
    fn has_events(&self, service: &Service, latest_revision: u64) -> bool {
        latest_revision > self.revision
            || !self.activity.is_current(&service.published_tab_activity())
            || self.terminals.iter().any(|(&tab, delivery)| {
                delivery.has_events(service.remote_v1_status(tab, &delivery.lease).ok())
            })
    }
    fn detach_terminals(self, service: &mut Service) {
        for (tab, delivery) in self.terminals {
            let _ = service.detach_attachment(tab, &delivery.lease);
        }
    }
}
pub struct RemoteState {
    pub facts: ManagerFacts,
    pub profiles: Option<wire::TerminalProfiles>,
    pub home: Option<wire::Directory>,
    views: HashMap<String, View>,
    transfers: Option<shellfox_remote::RemoteTransfers>,
    #[cfg(windows)]
    wsl_transfers: wsl_files::Transfers,
    visibility_dirty: bool,
    pub(super) stats: stats::PendingStats,
    closing: Option<close::Closing>,
    held: close::Held,
    /// Set by an attach that started its terminal; `keep` parks its request.
    starting: Option<shellfox_core::TabId>,
    /// Peer attaches waiting for their terminal's start, in arrival order.
    awaiting_start: Vec<(shellfox_core::TabId, shellfox_remote::OwnerRequest)>,
}
impl RemoteState {
    pub fn new(
        facts: ManagerFacts,
        profiles: Option<wire::TerminalProfiles>,
        home: Option<wire::Directory>,
    ) -> Self {
        Self {
            facts,
            profiles,
            home,
            views: HashMap::new(),
            transfers: None,
            #[cfg(windows)]
            wsl_transfers: Default::default(),
            visibility_dirty: false,
            stats: Default::default(),
            closing: None,
            held: Default::default(),
            starting: None,
            awaiting_start: Vec::new(),
        }
    }
    /// The oldest peer attach whose terminal start settled.
    pub(super) fn next_started(
        &mut self,
        service: &mut Service,
    ) -> Option<shellfox_remote::OwnerRequest> {
        self.awaiting_start
            .retain(|(_, request)| !request.is_cancelled());
        let index = self
            .awaiting_start
            .iter()
            .position(|(tab, _)| service.start_settled(*tab))?;
        Some(self.awaiting_start.remove(index).1)
    }
    pub(super) fn dispatch_request(
        &mut self,
        service: &mut Service,
        request: shellfox_remote::OwnerRequest,
    ) {
        let Some(request) = self.hold(service, request) else {
            return;
        };
        let Some(request) = self.stats_ready(service, request) else {
            return;
        };
        let cancel = request.cancellation_token();
        service.with_grant_recipient(cancel, |service| request.dispatch(&mut self.bind(service)));
        // A close this dispatch started may already be done.
        service.pump_closing();
        self.finish_close(service);
        if std::mem::take(&mut self.visibility_dirty) {
            service.activity.report_remote_visibility(
                self.views
                    .values()
                    .flat_map(|view| &view.visible)
                    .copied()
                    .collect(),
            );
        }
    }
    /// Wakes each view's pending long-poll once its next Poll has events.
    /// Runs on the owner thread after every pump and dispatch; extra wakes cost
    /// one empty poll, a missed one costs the full long-poll timeout.
    pub(super) fn wake_ready_views(&self, service: &Service) {
        for view in self.ready_views(service) {
            view.wake.notify();
        }
    }
    fn ready_views<'a>(&'a self, service: &'a Service) -> impl Iterator<Item = &'a View> {
        // Every metadata commit records `last_change` at the committed revision,
        // so the in-memory revision tells whether Poll would see a change.
        let latest = service.host().revision();
        self.views
            .values()
            .filter(move |view| view.has_events(service, latest))
    }
    pub fn bind<'a>(&'a mut self, service: &'a mut Service) -> RemoteService<'a> {
        RemoteService {
            state: self,
            service,
        }
    }
}
pub struct RemoteService<'a> {
    state: &'a mut RemoteState,
    service: &'a mut Service,
}
fn failure(code: ErrorCode, message: &str) -> AppError {
    AppError {
        code,
        message: message.into(),
        retryable: false,
    }
}
fn host_error(error: shellfox_core::Error) -> AppError {
    use shellfox_core::ErrorCode as Core;
    let code = match error.code {
        Core::Validation => ErrorCode::Validation,
        Core::NotFound => ErrorCode::NotFound,
        Core::Unsupported | Core::CleanupRequired => ErrorCode::Unsupported,
        _ => ErrorCode::StorageFailed,
    };
    failure(
        code,
        "Host metadata operation failed. No mutation was retried.",
    )
}
fn checked<T: serde::Serialize + Validate>(value: T) -> Result<serde_json::Value, AppError> {
    value
        .validate()
        .map_err(|_| failure(ErrorCode::Internal, "Invalid owner response."))?;
    serde_json::to_value(value)
        .map_err(|_| failure(ErrorCode::Internal, "Owner response encoding failed."))
}
fn key(actor: &Actor) -> String {
    format!("{}/{}", actor.peer_id, actor.client_id)
}
impl ServiceHost for RemoteService<'_> {
    /// OwnerRequest::dispatch holds approval through this synchronous call.
    /// Call that wrapper rather than invoking this method from network tasks.
    fn dispatch(&mut self, request: OwnerCommand) -> Result<serde_json::Value, AppError> {
        if !matches!(&request, OwnerCommand::Disconnect { .. }) {
            self.service.assert_owned().map_err(|_| {
                failure(ErrorCode::NativeUnavailable, "Native host ownership lost.")
            })?;
        }
        match request {
            OwnerCommand::Pair { actor } => {
                let key = key(&actor);
                if self.state.views.len() >= 32 && !self.state.views.contains_key(&key) {
                    return Err(failure(ErrorCode::Validation, "Remote view limit reached."));
                }
                if self
                    .state
                    .views
                    .get(&key)
                    .is_some_and(|view| view.epoch >= actor.view_epoch())
                {
                    return Err(failure(
                        ErrorCode::AuthFailed,
                        "A newer view is already paired.",
                    ));
                }
                let revision = self.service.host().snapshot().map_err(host_error)?.revision;
                if let Some(transfers) = &mut self.state.transfers {
                    transfers.cancel_actor(&key);
                }
                let controller = self.service.allocate_viewer_id().map_err(|_| {
                    failure(
                        ErrorCode::Internal,
                        "Terminal controller could not be allocated.",
                    )
                })?;
                if let Some(previous) = self.state.views.insert(
                    key.clone(),
                    View {
                        epoch: actor.view_epoch(),
                        revision,
                        visible: vec![],
                        controller,
                        terminals: HashMap::new(),
                        replays: HashMap::new(),
                        wake: actor.poll_wake(),
                        activity: v1_activity::Reported::new(self.service.published_tab_activity()),
                    },
                ) {
                    #[cfg(windows)]
                    self.state.wsl_transfers.cancel_view(&key, previous.epoch);
                    self.state.visibility_dirty |= !previous.visible.is_empty();
                    previous.detach_terminals(self.service);
                }
                Ok(serde_json::json!({"paired":true,"terminalControl":"exclusive-v1"}))
            }
            OwnerCommand::Disconnect { actor } => {
                if self
                    .state
                    .views
                    .get(&key(&actor))
                    .is_some_and(|view| view.epoch == actor.view_epoch())
                {
                    if let Some(view) = self.state.views.remove(&key(&actor)) {
                        self.state.visibility_dirty |= !view.visible.is_empty();
                        view.detach_terminals(self.service);
                    }
                    #[cfg(windows)]
                    self.state.wsl_transfers.cancel_actor(&actor);
                    if let Some(transfers) = &mut self.state.transfers {
                        transfers.cancel_actor(&key(&actor));
                    }
                }
                Ok(serde_json::json!({"disconnected":true}))
            }
            OwnerCommand::Poll { actor } => {
                let activity = self.service.published_tab_activity();
                let private = match self.state.views.get(&key(&actor)) {
                    Some(view) if !view.activity.is_current(&activity) => Some(
                        self.private_tabs(&self.service.host().snapshot().map_err(host_error)?),
                    ),
                    _ => None,
                };
                let view = self
                    .state
                    .views
                    .get_mut(&key(&actor))
                    .filter(|view| view.epoch == actor.view_epoch())
                    .ok_or_else(|| failure(ErrorCode::AuthFailed, "Pair this view first."))?;
                let changed = self
                    .service
                    .host()
                    .changed_since(view.revision)
                    .map_err(host_error)?;
                let mut events = vec![];
                if let Some(mut event) = changed {
                    // Local CLI focus never redirects remote input.
                    event.select_session_id = None;
                    view.revision = event.revision;
                    events.push(adapter::changed_event(&event).map_err(AppError::from)?);
                }
                if let Some(private) = private {
                    let revision = view.revision;
                    view.activity
                        .report(activity, &private, revision, &mut events);
                }
                let quota = 64 / view.terminals.len().max(1);
                for (&tab, delivery) in &mut view.terminals {
                    delivery.poll(self.service, tab, quota, &mut events);
                }
                checked(wire::PollResponse { events })
            }
            OwnerCommand::Execute { actor, command } => {
                command
                    .validate()
                    .map_err(|_| failure(ErrorCode::Validation, "Invalid remote command."))?;
                if !self
                    .state
                    .views
                    .get(&key(&actor))
                    .is_some_and(|view| view.epoch == actor.view_epoch())
                {
                    return Err(failure(ErrorCode::AuthFailed, "Pair this view first."));
                }
                self.authorize_profiles(&command)?;
                self.execute(&actor, command)
            }
        }
    }
    fn keep(
        &mut self,
        request: shellfox_remote::OwnerRequest,
    ) -> Option<shellfox_remote::OwnerRequest> {
        let request = self.keep_closing(request)?;
        match self.state.starting.take() {
            Some(tab) => {
                self.state.awaiting_start.push((tab, request));
                None
            }
            None => Some(request),
        }
    }
}
