use super::*;
pub(super) fn execute(
    state: &mut RemoteState,
    service: &mut Service,
    connection: &ObserverConnection,
    command: v2::Command,
) -> Result<serde_json::Value, AppError> {
    use v2::Command::*;
    let view_id = connection.binding().connection_id;
    match command {
        AttachTerminal(input) => attachment::attach(service, state, connection, input),
        AcknowledgeTerminal(input) => {
            delivery_metadata(state, service, view_id, &input.delivery)?;
            runtime::acknowledge(
                service,
                state
                    .observer_views
                    .entries
                    .get_mut(&view_id)
                    .ok_or_else(|| failure(ErrorCode::AuthFailed, "Observer view missing."))?,
                input,
            )
        }
        DetachTerminal(input) => runtime::detach(
            service,
            state
                .observer_views
                .entries
                .get_mut(&view_id)
                .ok_or_else(|| failure(ErrorCode::AuthFailed, "Observer view missing."))?,
            input,
        ),
        command => metadata::execute(state, service, connection.actor(), view_id, command),
    }
}
fn delivery_metadata(
    state: &mut RemoteState,
    service: &mut Service,
    view_id: v2::Id,
    reference: &v2::DeliveryRef,
) -> Result<(), AppError> {
    let attached = state
        .observer_views
        .entries
        .get(&view_id)
        .and_then(|view| view.deliveries.get(&reference.delivery_id))
        .filter(|attached| &attached.reference == reference)
        .cloned()
        .ok_or_else(|| {
            failure(
                ErrorCode::AuthFailed,
                "Observer delivery is not in this connection.",
            )
        })?;
    if let Err(error) = tab_target(state, service, attached.identity.tab_id) {
        if let Some(current) = state
            .observer_views
            .entries
            .get_mut(&view_id)
            .and_then(|view| view.deliveries.get_mut(&reference.delivery_id))
        {
            current.revoked = true;
        }
        let _ = service.retire_remote_observer(attached.identity, attached.observer);
        return Err(error);
    }
    Ok(())
}
