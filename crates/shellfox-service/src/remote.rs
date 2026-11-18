//! Remote dispatch borrows the existing service; it never opens a second Host.
#[cfg(test)]
#[path = "remote_legacy_scope_tests.rs"]
mod legacy_scope_tests;
#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
use crate::Service;
use shellfox_core::{SessionId, Snapshot};
use shellfox_protocol::{self as wire, AppError, ErrorCode, RemoteCommand, Validate};
use shellfox_remote::{
    adapter::{self, ManagerFacts},
    capability_unavailable, Actor, OwnerCommand, ServiceHost,
};
use std::collections::HashMap;

#[path = "remote_commands.rs"]
mod commands;
#[path = "remote_dispatcher.rs"]
mod dispatcher;
#[path = "remote_files.rs"]
mod files;
#[path = "manager_owner.rs"]
mod manager;
#[path = "remote_v2_manager.rs"]
mod observer;
#[path = "remote_privacy.rs"]
mod privacy;
#[path = "remote_stats.rs"]
mod stats;
pub use dispatcher::RemoteDispatcher;
#[path = "remote_facts.rs"]
pub mod facts;
#[path = "remote_source.rs"]
mod source;
pub use facts::{
    ManagerFactsFuture, ManagerFactsProvider, ManagerFactsRequest, ManagerFactsResult,
    RuntimeFactIdentity,
};
pub use source::{SourceFactsInstaller, SourceFactsProjector, SourceFactsProviderSlot};
#[path = "remote_host.rs"]
mod host;
pub use host::RemoteBackend;
#[path = "remote_setup.rs"]
mod setup;
pub use setup::{
    RemoteBootResources, RemoteHostSetup, RemoteResources, RemoteSetupOutcome,
    RemoteUnavailableReason,
};
#[path = "outgoing_v2.rs"]
pub(crate) mod outgoing;
pub use outgoing::{OutgoingV2Endpoint, OutgoingV2Future, OutgoingV2Response};
#[path = "outgoing_v2_wire.rs"]
pub(crate) mod outgoing_wire;
#[path = "remote_operator_wire.rs"]
pub(crate) mod operator;
pub use operator::{RemoteOperatorPending,RemoteOperatorState};
#[path = "remote_native.rs"]
pub mod native;
#[path = "remote_preferences.rs"]
pub mod preferences;
#[path = "remote_runtime.rs"]
pub mod runtime;
pub use native::{AuthKey, RemoteControl, RemoteControlState, RemoteRequest, RemoteResponse};
pub use runtime::{GoNetworkFactory, RemoteNetwork, RemoteNetworkFactory, RemoteRuntime};
/// Bounded loopback-only boot setting, never a peer request URL.
pub use shellfox_transport::LoopbackOrigin as ControlServerOrigin;
pub(super) struct MetadataV2 {
    pub revision: u64,
    pub visible: Vec<SessionId>,
}
struct View {
    epoch: u64,
    revision: u64,
    visible: Vec<SessionId>,
}
pub struct RemoteState {
    pub facts: ManagerFacts,
    pub profiles: Option<wire::TerminalProfiles>,
    pub home: Option<wire::Directory>,
    views: HashMap<String, View>,
    transfers: Option<shellfox_remote::RemoteTransfers>,
    observer_views: observer::ObserverViews,
    manager_views: manager::ManagerViews,
    manager_uploads: Option<shellfox_remote::terminal_upload::UploadInbox>,
    metadata_v2: HashMap<wire::observer_v2::Id, MetadataV2>,
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
            observer_views: observer::ObserverViews::default(),
            manager_views: manager::ManagerViews::default(),
            manager_uploads: None,
            metadata_v2: HashMap::new(),
        }
    }
    pub(super) fn dispatch_request(
        &mut self,
        service: &mut Service,
        request: shellfox_remote::OwnerRequest,
    ) {
        let cancel = request.cancellation_token();
        service.with_grant_recipient(cancel, |service| request.dispatch(&mut self.bind(service)));
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
        if !matches!(
            &request,
            OwnerCommand::Disconnect { .. }
                | OwnerCommand::ObserverV2(_)
                | OwnerCommand::ManagerV2(_)
        ) {
            self.service.assert_owned().map_err(|_| {
                failure(ErrorCode::NativeUnavailable, "Native host ownership lost.")
            })?;
        }
        match request {
            OwnerCommand::ObserverV2(request) => {
                observer::handle(self.state, self.service, *request)
            }
            OwnerCommand::ManagerV2(request) => manager::handle(self.state, self.service, *request),
            OwnerCommand::Pair { actor } => {
                if self.state.views.len() >= 32 && !self.state.views.contains_key(&key(&actor)) {
                    return Err(failure(ErrorCode::Validation, "Remote view limit reached."));
                }
                let revision = self.service.host().snapshot().map_err(host_error)?.revision;
                if let Some(transfers) = &mut self.state.transfers {
                    transfers.cancel_actor(&key(&actor));
                }
                self.state.views.insert(
                    key(&actor),
                    View {
                        epoch: actor.view_epoch(),
                        revision,
                        visible: vec![],
                    },
                );
                Ok(serde_json::json!({"paired":true,"terminalControl":"exclusive-v1"}))
            }
            OwnerCommand::Disconnect { actor } => {
                if self
                    .state
                    .views
                    .get(&key(&actor))
                    .is_some_and(|view| view.epoch == actor.view_epoch())
                {
                    self.state.views.remove(&key(&actor));
                    if let Some(transfers) = &mut self.state.transfers {
                        transfers.cancel_actor(&key(&actor));
                    }
                }
                Ok(serde_json::json!({"disconnected":true}))
            }
            OwnerCommand::Poll { actor } => {
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
}
