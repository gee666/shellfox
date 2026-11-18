//! V2 dispatch on the EXISTING RemoteState and Service owner, never another Host.
use super::*;
use shellfox_protocol::observer_v2 as v2;
use shellfox_remote::{ObserverConnection, ObserverOperation, ObserverOwnerRequest};
#[path = "remote_v2_attach.rs"]
mod attachment;
#[path = "remote_v2_catalog.rs"]
mod catalog;
#[path = "remote_v2_dispatch.rs"]
mod dispatch;
#[path = "remote_v2_launch.rs"]
pub(super) mod launch;
#[path = "remote_v2_metadata.rs"]
pub(super) mod metadata;
#[cfg(test)]
#[path = "remote_v2_native_allow_tests.rs"]
mod native_allow_tests;
#[path = "remote_v2_runtime.rs"]
pub(super) mod runtime;
#[path = "remote_v2_settings.rs"]
mod settings;
#[path = "remote_v2_catalog_tabs.rs"]
mod tabs;
#[cfg(test)]
#[path = "remote_v2_catalog_tests.rs"]
mod tests;

#[derive(Default)]
pub(super) struct ObserverViews {
    entries: HashMap<v2::Id, ObserverView>,
}
struct ObserverView {
    connection: ObserverConnection,
    deliveries: HashMap<v2::Id, runtime::Attached>,
}
pub(super) fn handle(
    state: &mut RemoteState,
    service: &mut Service,
    request: ObserverOwnerRequest,
) -> Result<serde_json::Value, AppError> {
    let id = request.connection.binding().connection_id;
    if !matches!(request.operation, ObserverOperation::Disconnect) {
        service
            .assert_owned()
            .map_err(|_| failure(ErrorCode::NativeUnavailable, "Native owner was lost."))?;
    }
    match request.operation {
        ObserverOperation::Pair => {
            let replaced = state
                .observer_views
                .entries
                .iter()
                .filter(|(previous_id, view)| {
                    **previous_id != id
                        && view.connection.actor().peer_id == request.connection.actor().peer_id
                        && view.connection.binding().client_id
                            == request.connection.binding().client_id
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for previous_id in replaced {
                if let Some(view) = state.observer_views.entries.get_mut(&previous_id) {
                    runtime::detach_all(service, view)?;
                }
                state.observer_views.entries.remove(&previous_id);
                state.metadata_v2.remove(&previous_id);
            }
            if state.observer_views.entries.len() >= 32
                && !state.observer_views.entries.contains_key(&id)
            {
                return Err(failure(
                    ErrorCode::Validation,
                    "Observer owner view limit reached.",
                ));
            }
            state.metadata_v2.insert(
                id,
                MetadataV2 {
                    revision: service.host().snapshot().map_err(host_error)?.revision,
                    visible: Vec::new(),
                },
            );
            state.observer_views.entries.insert(
                id,
                ObserverView {
                    connection: request.connection.clone(),
                    deliveries: HashMap::new(),
                },
            );
            checked(request.connection.selection().clone())
        }
        ObserverOperation::Disconnect => {
            if let Some(view) = state.observer_views.entries.get_mut(&id) {
                runtime::detach_all(service, view)?;
            }
            state.observer_views.entries.remove(&id);
            state.metadata_v2.remove(&id);
            if let Some(transfers) = &mut state.transfers {
                transfers.cancel_actor(&key(request.connection.actor()));
            }
            checked(v2::Disconnected { disconnected: true })
        }
        ObserverOperation::Execute(command) => {
            command
                .authorize_profile()
                .map_err(|e| failure(e.code, &e.message))?;
            let view = state.observer_views.entries.get(&id).ok_or_else(|| {
                failure(ErrorCode::AuthFailed, "Pair the observer owner view first.")
            })?;
            if view.connection.binding() != request.connection.binding()
                || view.connection.actor().approval_epoch
                    != request.connection.actor().approval_epoch
            {
                return Err(failure(
                    ErrorCode::AuthFailed,
                    "Observer binding was replaced.",
                ));
            }
            dispatch::execute(state, service, &request.connection, command)
        }
        ObserverOperation::Poll => {
            let snapshot = service.host().snapshot().map_err(host_error)?;
            let allowed = snapshot
                .sessions
                .iter()
                .flat_map(|session| session.tabs.iter())
                .filter(|tab| tab_visible(state, service, tab))
                .map(|tab| tab.id)
                .collect::<std::collections::HashSet<_>>();
            let view = state
                .observer_views
                .entries
                .get_mut(&id)
                .ok_or_else(|| failure(ErrorCode::AuthFailed, "Observer view is detached."))?;
            let mut events = Vec::new();
            let revision = state
                .metadata_v2
                .get(&id)
                .ok_or_else(|| failure(ErrorCode::AuthFailed, "Metadata view missing."))?
                .revision;
            if let Some(mut changed) = service.host().changed_since(revision).map_err(host_error)? {
                changed.select_session_id = None;
                let wire::HostEvent::Changed { event } =
                    adapter::changed_event(&changed).map_err(AppError::from)?
                else {
                    return Err(failure(ErrorCode::Internal, "Invalid changed event."));
                };
                events.push(v2::HostEvent::Changed { event });
            }
            runtime::poll(service, view, &snapshot, &allowed, &mut events)?;
            checked(v2::PollResponse { events })
        }
    }
}
fn tab_visible(state: &RemoteState, service: &Service, tab: &shellfox_core::Tab) -> bool {
    if service.terminals.get(&tab.id).is_some_and(|terminal| {
        terminal.runtime.identity().session_id == tab.session_id
            && terminal.runtime.launch_privilege_facts().privilege
                == shellfox_runtime::LaunchPrivilege::Elevated
    }) {
        return false;
    }
    !state.profiles.as_ref().is_some_and(|profiles| {
        profiles
            .profiles
            .iter()
            .any(|profile| profile.id == tab.profile_id.as_str() && profile.elevation.is_some())
    })
}
fn authorize_session_sources(
    state: &RemoteState,
    service: &Service,
    session: &shellfox_core::Session,
) -> Result<(), AppError> {
    if session
        .tabs
        .iter()
        .any(|tab| !tab_visible(state, service, tab))
    {
        return Err(failure(
            ErrorCode::AccessDenied,
            "This session operation would affect a local-only source.",
        ));
    }
    Ok(())
}
fn target(
    _state: &RemoteState,
    service: &Service,
    id: SessionId,
) -> Result<shellfox_core::Session, AppError> {
    let snapshot = service.host().snapshot().map_err(host_error)?;
    snapshot
        .sessions
        .into_iter()
        .find(|session| session.id == id)
        .ok_or_else(|| failure(ErrorCode::NotFound, "Authorized session not found."))
}
pub(super) fn tab_target(
    state: &RemoteState,
    service: &Service,
    tab: shellfox_core::TabId,
) -> Result<shellfox_core::Session, AppError> {
    let snapshot = service.host().snapshot().map_err(host_error)?;
    snapshot
        .sessions
        .into_iter()
        .find(|session| {
            session
                .tabs
                .iter()
                .any(|value| value.id == tab && tab_visible(state, service, value))
        })
        .ok_or_else(|| failure(ErrorCode::NotFound, "Authorized tab not found."))
}
