//! manager-v2 operations on the existing RemoteState, Service and owner channel.
use super::*;
use crate::manager_delivery::Role;
use shellfox_protocol::manager_v2 as m;
use shellfox_remote::{ManagerConnection, ManagerOperation, ManagerOwnerRequest};
#[path = "manager_attach.rs"]
mod attach;
#[path = "manager_control.rs"]
mod control;
#[path = "manager_input.rs"]
mod input;
#[path = "manager_poll.rs"]
mod poll;
#[path = "manager_uploads.rs"]
mod uploads;
#[derive(Default)]
pub(super) struct ManagerViews {
    entries: HashMap<m::Id, ManagerView>,
}
struct ManagerView {
    connection: ManagerConnection,
    sources: HashMap<m::Id, Source>,
    inputs: std::collections::VecDeque<m::Id>,
}
#[derive(Clone)]
struct Source {
    identity: shellfox_runtime::Identity,
    role: Role,
    delivery: m::DeliveryRef,
    control: Option<m::ControlRef>,
    layout: Option<m::LayoutRef>,
    policy: u64,
    revoked: bool,
}
pub(super) fn handle(
    state: &mut RemoteState,
    service: &mut Service,
    request: ManagerOwnerRequest,
) -> Result<serde_json::Value, AppError> {
    let id = request.connection.binding().connection_id;
    let failure_sink = request.failure.clone();
    if !matches!(request.operation, ManagerOperation::Disconnect) {
        service
            .assert_owned()
            .map_err(|_| failure(ErrorCode::NativeUnavailable, "Host ownership was lost."))?;
    }
    match request.operation {
        ManagerOperation::Pair => {
            let old = state
                .manager_views
                .entries
                .iter()
                .filter(|(old, view)| {
                    **old != id
                        && view.connection.actor().peer_id == request.connection.actor().peer_id
                        && view.connection.binding().client_id
                            == request.connection.binding().client_id
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for old in old {
                disconnect(state, service, old)?;
            }
            if state.manager_views.entries.len() >= 32
                && !state.manager_views.entries.contains_key(&id)
            {
                return Err(failure(
                    ErrorCode::Validation,
                    "Manager connection limit reached.",
                ));
            }
            state.metadata_v2.insert(
                id,
                MetadataV2 {
                    revision: service.host().snapshot().map_err(host_error)?.revision,
                    visible: Vec::new(),
                },
            );
            state.manager_views.entries.insert(
                id,
                ManagerView {
                    connection: request.connection.clone(),
                    sources: HashMap::new(),
                    inputs: std::collections::VecDeque::new(),
                },
            );
            checked(request.connection.selection().clone())
        }
        ManagerOperation::Disconnect => {
            disconnect(state, service, id)?;
            checked(m::Disconnected { disconnected: true })
        }
        ManagerOperation::Poll => poll::poll(state, service, id),
        ManagerOperation::Execute(command) => {
            let view =
                state.manager_views.entries.get(&id).ok_or_else(|| {
                    failure(ErrorCode::AuthFailed, "Pair this manager view first.")
                })?;
            if view.connection.binding() != request.connection.binding() {
                return Err(failure(
                    ErrorCode::AuthFailed,
                    "Manager binding was replaced.",
                ));
            }
            if let Some(compatible) = shellfox_remote::manager_policy::compatible(&command) {
                return observer::metadata::execute(
                    state,
                    service,
                    request.connection.actor(),
                    id,
                    compatible,
                );
            }
            match command {
                m::Command::AttachTerminal(value) => {
                    attach::attach(state, service, &request.connection, value, false)
                }
                m::Command::TakeTerminalControl(value) => {
                    attach::take(state, service, &request.connection, value)
                }
                m::Command::AcknowledgeTerminal(value) => control::ack(state, service, id, value),
                m::Command::DetachTerminal(value) => control::detach(state, service, id, value),
                m::Command::WriteTerminal(value) => input::write(state, service, id, value)
                    .map_err(|error| failure_sink.report(error)),
                m::Command::ResizeTerminal(value) => control::resize(state, service, id, value),
                command @ (m::Command::BeginTerminalUpload(_)
                | m::Command::WriteTerminalUpload(_)
                | m::Command::FinishTerminalUpload(_)
                | m::Command::CancelTerminalUpload(_)) => {
                    uploads::request(state, service, id, command)
                }
                command => Err(failure(
                    ErrorCode::Validation,
                    &format!(
                        "Typed manager dispatcher mismatch for {}.",
                        command.method_name()
                    ),
                )),
            }
        }
    }
}
fn disconnect(state: &mut RemoteState, service: &mut Service, id: m::Id) -> Result<(), AppError> {
    // Invalidate the whole connection BEFORE any bounded cleanup admission.
    if let Some(view) = state.manager_views.entries.get_mut(&id) {
        #[cfg(windows)]
        state.wsl_transfers.cancel_actor(view.connection.actor());
        for source in view.sources.values_mut() {
            source.revoked = true;
            service.manager_v2_mailboxes.revoke(
                source.identity,
                source.role,
                m::RevokedReason::ViewDetached,
            );
            service.manager_v2_mailboxes.forget_notice(&source.delivery);
        }
    }
    let cleanup = match &mut state.manager_uploads {
        Some(inbox) => inbox
            .cancel_actor(&uploads::owner_id(id))
            .map_err(uploads::file_error),
        None => Ok(()),
    };
    let retirement = service
        .pump_manager_retirements()
        .map_err(|_| native_error());
    state.manager_views.entries.remove(&id);
    state.remove_metadata_view(id);
    cleanup.and(retirement)
}
fn source_for<'a>(
    view: &'a mut ManagerView,
    delivery: &m::DeliveryRef,
) -> Result<&'a mut Source, AppError> {
    view.sources
        .get_mut(&delivery.delivery_id)
        .filter(|source| source.delivery == *delivery)
        .ok_or_else(|| {
            failure(
                ErrorCode::AuthFailed,
                "Delivery is not in this manager connection.",
            )
        })
}
fn control_for(view: &mut ManagerView, control: &m::ControlRef) -> Result<Source, AppError> {
    view.sources
        .values()
        .find(|source| source.control.as_ref() == Some(control) && !source.revoked)
        .cloned()
        .ok_or_else(|| {
            failure(
                ErrorCode::AuthFailed,
                "Control is not in this manager connection.",
            )
        })
}
fn allowed(state: &RemoteState, service: &Service, source: &Source) -> bool {
    !source.revoked
        && observer::tab_target(state, service, source.identity.tab_id).is_ok()
        && observer::runtime::effective_policy(service, source.identity) == Some(source.policy)
}
fn retire(
    service: &mut Service,
    source: &Source,
    reason: m::RevokedReason,
) -> Result<(), AppError> {
    service
        .manager_v2_mailboxes
        .revoke(source.identity, source.role, reason);
    service.pump_manager_retirements().map_err(|_| {
        failure(
            ErrorCode::NativeUnavailable,
            "Manager lease retirement retained; source was not stopped.",
        )
    })
}
fn native_error() -> AppError {
    failure(
        ErrorCode::NativeUnavailable,
        "Runtime operation outcome is uncertain; input was not retried.",
    )
}
