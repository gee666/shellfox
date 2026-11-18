use super::*;
use crate::grant_requests::GrantHolder;
use crate::remote_v2::delivery::ObserverDelivery;
use shellfox_runtime::Command;
pub(super) fn attach(
    service: &mut Service,
    state: &mut RemoteState,
    connection: &ObserverConnection,
    input: v2::AttachTerminal,
) -> Result<serde_json::Value, AppError> {
    let tab = input
        .tab_id
        .to_string()
        .parse()
        .map_err(|_| failure(ErrorCode::Validation, "Invalid tab identity."))?;
    tab_target(state, service, tab)?;
    let Some(terminal) = service.terminals.get(&tab) else {
        return unavailable(input.tab_id, v2::UnavailableReason::SourceUnavailable);
    };
    let identity = terminal.runtime.identity();
    if input
        .resume
        .as_ref()
        .is_some_and(|resume| resume.generation.to_string() != identity.generation.to_string())
    {
        return Err(failure(
            ErrorCode::TargetLost,
            "Observer generation changed.",
        ));
    }
    let Some(policy_epoch) = runtime::effective_policy(service, identity) else {
        return unavailable(input.tab_id, v2::UnavailableReason::FactsUnavailable);
    };
    let view_id = connection.binding().connection_id;
    let view = state
        .observer_views
        .entries
        .get_mut(&view_id)
        .ok_or_else(|| failure(ErrorCode::AuthFailed, "Observer view missing."))?;
    let previous = view
        .deliveries
        .values()
        .filter(|attached| attached.identity.tab_id == tab)
        .cloned()
        .collect::<Vec<_>>();
    for attached in previous {
        if let Some(current) = view.deliveries.get_mut(&attached.reference.delivery_id) {
            current.revoked = true;
        }
        service
            .retire_remote_observer(attached.identity, attached.observer)
            .map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Old observer retirement retained.",
                )
            })?;
        view.deliveries.remove(&attached.reference.delivery_id);
    }
    if !service.remote_v2_mailboxes.can_insert() {
        return unavailable(input.tab_id, v2::UnavailableReason::FactsUnavailable);
    }
    let viewer = service.allocate_viewer_id().map_err(|_| {
        failure(
            ErrorCode::NativeUnavailable,
            "Owner viewer allocation exhausted.",
        )
    })?;
    let offer = service.observer_grant(tab, viewer).map_err(|_| {
        failure(
            ErrorCode::NativeUnavailable,
            "Observer grant remains under authoritative Service custody; no retry was made.",
        )
    })?;
    if offer.identity() != identity {
        return Err(failure(
            ErrorCode::TargetLost,
            "Observer source changed before recovery.",
        ));
    }
    let observer = offer.lease();
    let entry = match ObserverDelivery::new(identity, observer) {
        Ok(entry) => entry,
        Err(_) => {
            // Offer drop retains exact cleanup in the central Service ledger.
            return unavailable(input.tab_id, v2::UnavailableReason::RecoveryUnavailable);
        }
    };
    if service.remote_v2_mailboxes.insert_offer(entry).is_err() {
        return unavailable(input.tab_id, v2::UnavailableReason::RecoveryUnavailable);
    }
    let response = service.call(
        tab,
        Command::ObserverRecoverStamped {
            observer,
            capability: shellfox_runtime::AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
    );
    let recovery = match response.ok().and_then(|response| {
        service
            .remote_v2_mailboxes
            .get_mut(identity, observer)
            .and_then(|entry| entry.recover(response).ok())
    }) {
        Some(recovery) => recovery,
        None => {
            discard(service, identity, observer);
            return unavailable(input.tab_id, v2::UnavailableReason::RecoveryUnavailable);
        }
    };
    if runtime::effective_policy(service, identity) != Some(policy_epoch) {
        discard(service, identity, observer);
        return unavailable(input.tab_id, v2::UnavailableReason::FactsUnavailable);
    }
    let reference = service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .ok_or_else(|| {
            failure(
                ErrorCode::TargetLost,
                "Observer disappeared during recovery.",
            )
        })?
        .reference()
        .clone();
    let result = match checked(v2::Attachment::Attached {
        delivery: reference.clone(),
        input: (),
        layout: v2::Layout::FixedCanonicalGrid,
        recovery,
    }) {
        Ok(value) => value,
        Err(error) => {
            discard(service, identity, observer);
            return Err(error);
        }
    };
    let binding = connection.binding();
    let current = state
        .observer_views
        .entries
        .get(&view_id)
        .is_some_and(|view| view.connection.binding() == binding)
        && shellfox_protocol::AuthenticatedPeerId::new(connection.actor().peer_id.clone())
            .ok()
            .is_some_and(|peer| connection.authenticated().check(&peer, &binding).is_ok());
    if !current {
        discard(service, identity, observer);
        return Err(failure(
            ErrorCode::AuthFailed,
            "Observer binding changed before adoption.",
        ));
    }
    state
        .observer_views
        .entries
        .get_mut(&view_id)
        .ok_or_else(|| failure(ErrorCode::AuthFailed, "Observer connection replaced."))?
        .deliveries
        .insert(
            reference.delivery_id,
            runtime::Attached {
                identity,
                observer,
                reference: reference.clone(),
                policy_epoch,
                revoked: false,
            },
        );
    if service
        .adopt_observer_grant(offer, GrantHolder::ObserverMailbox)
        .is_err()
    {
        if let Some(view) = state.observer_views.entries.get_mut(&view_id) {
            view.deliveries.remove(&reference.delivery_id);
        }
        discard(service, identity, observer);
        return Err(failure(
            ErrorCode::NativeUnavailable,
            "Observer grant was not adopted; central retirement retained.",
        ));
    }
    Ok(result)
}
fn discard(
    service: &mut Service,
    identity: shellfox_runtime::Identity,
    observer: shellfox_runtime::ObserverLease,
) {
    service
        .remote_v2_mailboxes
        .revoke_delivery(identity, observer);
    let _ = service
        .remote_v2_mailboxes
        .discard_unadopted(identity, observer);
}
fn unavailable(
    tab_id: v2::Id,
    reason: v2::UnavailableReason,
) -> Result<serde_json::Value, AppError> {
    checked(v2::Attachment::Unavailable { tab_id, reason })
}
