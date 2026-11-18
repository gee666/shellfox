//! Missing-source creation is explicit Manager control intent, never observer recovery.
use super::*;
#[cfg(test)]
#[path = "manager_cold_attach_tests.rs"]
mod tests;

fn current(state: &RemoteState, connection: &ManagerConnection) -> bool {
    let binding = connection.binding();
    connection.actor().permission == shellfox_remote::ViewPermission::Write
        && state
            .manager_views
            .entries
            .get(&binding.connection_id)
            .is_some_and(|view| {
                view.connection.binding() == binding
                    && view.connection.actor().approval_epoch == connection.actor().approval_epoch
                    && view.connection.actor().peer_id == connection.actor().peer_id
            })
        && shellfox_protocol::AuthenticatedPeerId::new(connection.actor().peer_id.clone())
            .ok()
            .is_some_and(|peer| connection.authenticated().check(&peer, &binding).is_ok())
}
fn new_source_intent(input: &m::AttachTerminal, takeover: bool, format: m::RecoveryFormat) -> bool {
    input.resume.is_none()
        && !takeover
        && matches!(input.control, m::ControlRequest::IfFree)
        && format == m::RecoveryFormat::Native0RawV1
}
fn recorded_target(
    service: &Service,
    session: &shellfox_core::Session,
    tab: shellfox_core::TabId,
) -> Result<bool, AppError> {
    let Some(target) = session.tabs.iter().find(|value| value.id == tab) else {
        return Ok(false);
    };
    if target.state != shellfox_core::TabState::Pending || target.created_at.is_none() {
        return Ok(false);
    }
    let recorded = service
        .host()
        .session_creation_facts()
        .map_err(host_error)?;
    Ok(recorded.get(&session.id).is_some_and(|facts| {
        facts.validate().is_ok() && facts.adapter_id == shellfox_core::SettingsAdapter::EmbeddedPty
    }))
}
// ManagerOwnerRequest::dispatch holds the ORIGINAL approval/current-connection
// guards through this synchronous startup. Never recursively acquire those locks.
pub(super) fn start(
    state: &RemoteState,
    service: &mut Service,
    connection: &ManagerConnection,
    input: &m::AttachTerminal,
    tab: shellfox_core::TabId,
    takeover: bool,
) -> Result<bool, AppError> {
    if input.resume.is_some() {
        return Err(failure(
            ErrorCode::TargetLost,
            "The requested Manager generation is not running.",
        ));
    }
    if !new_source_intent(input, takeover, connection.selection().recovery_format) {
        return Ok(false);
    }
    if service.grant_recipient.is_cancelled() || !current(state, connection) {
        return Err(failure(
            ErrorCode::AuthFailed,
            "Manager creation authority is no longer current.",
        ));
    }
    let original = observer::tab_target(state, service, tab)?;
    let target = original
        .tabs
        .iter()
        .find(|value| value.id == tab)
        .ok_or_else(native_error)?;
    if !recorded_target(service, &original, tab)? {
        return Ok(false);
    }
    observer::launch::ensure_launch_profile(state, target.profile_id.as_str())?;
    if !service.start_manager_native0_terminal(tab).map_err(|_| {
        failure(
            ErrorCode::NativeUnavailable,
            "Manager source startup was not confirmed; the original owner remains retained.",
        )
    })? {
        return Ok(false);
    }
    let identity = service
        .terminals
        .get(&tab)
        .ok_or_else(native_error)?
        .runtime
        .identity();
    let revalidated = observer::tab_target(state, service, tab)
        .is_ok_and(|session| session == original)
        && !service.grant_recipient.is_cancelled()
        && current(state, connection);
    if !revalidated || observer::runtime::effective_policy(service, identity).is_none() {
        let cleanup = service.stop_terminal(shellfox_core::TerminalIdentity {
            tab_id: tab,
            generation: identity.generation,
        });
        if cleanup.is_err() {
            return Err(failure(ErrorCode::NativeUnavailable,"Rejected Manager startup cleanup is unconfirmed; original Runtime remains retained."));
        }
        return Err(failure(if revalidated{ErrorCode::AccessDenied}else{ErrorCode::TargetLost},
            "New Manager source did not retain its original metadata, approval/binding or sealed launch eligibility."));
    }
    Ok(true)
}
