use super::*;
use crate::grant_requests::{ControllerGrantOffer, GrantFailure, GrantHolder, ObserverGrantOffer};
use crate::manager_delivery::ManagerDelivery;
use shellfox_runtime::Command;
#[path = "manager_cold_attach.rs"]
mod cold;
enum Offer {
    Controller(ControllerGrantOffer),
    Observer(ObserverGrantOffer),
}
impl Offer {
    fn identity(&self) -> shellfox_runtime::Identity {
        match self {
            Self::Controller(offer) => offer.identity(),
            Self::Observer(offer) => offer.identity(),
        }
    }
    fn role(&self) -> Role {
        match self {
            Self::Controller(offer) => Role::Controller(offer.lease()),
            Self::Observer(offer) => Role::Observer(offer.lease()),
        }
    }
    fn adopt(self, service: &mut Service) -> anyhow::Result<()> {
        match self {
            Self::Controller(offer) => service
                .adopt_controller_grant(offer, GrantHolder::ManagerMailbox)
                .map(|_| ()),
            Self::Observer(offer) => service
                .adopt_observer_grant(offer, GrantHolder::ManagerMailbox)
                .map(|_| ()),
        }
    }
}
pub(super) fn take(
    state: &mut RemoteState,
    service: &mut Service,
    connection: &ManagerConnection,
    input: m::TakeTerminalControl,
) -> Result<serde_json::Value, AppError> {
    let id = connection.binding().connection_id;
    let source = source_for(
        state
            .manager_views
            .entries
            .get_mut(&id)
            .ok_or_else(|| failure(ErrorCode::AuthFailed, "Manager view missing."))?,
        &input.delivery,
    )?
    .clone();
    attach(
        state,
        service,
        connection,
        m::AttachTerminal {
            tab_id: source.delivery.tab_id,
            resume: Some(m::Resume {
                generation: source.delivery.generation,
                after_sequence: 0,
            }),
            control: m::ControlRequest::IfFree,
        },
        true,
    )
}
pub(super) fn attach(
    state: &mut RemoteState,
    service: &mut Service,
    connection: &ManagerConnection,
    input: m::AttachTerminal,
    takeover: bool,
) -> Result<serde_json::Value, AppError> {
    let tab = input
        .tab_id
        .to_string()
        .parse()
        .map_err(|_| failure(ErrorCode::Validation, "Invalid tab."))?;
    observer::tab_target(state, service, tab)?;
    if !service.terminals.contains_key(&tab)
        && !cold::start(state, service, connection, &input, tab, takeover)?
    {
        return unavailable(input.tab_id, m::UnavailableReason::SourceUnavailable);
    }
    let terminal = service.terminals.get(&tab).ok_or_else(native_error)?;
    let identity = terminal.runtime.identity();
    if input
        .resume
        .as_ref()
        .is_some_and(|resume| resume.generation.to_string() != identity.generation.to_string())
    {
        return Err(failure(
            ErrorCode::TargetLost,
            "Manager source generation changed.",
        ));
    }
    let Some(policy) = observer::runtime::effective_policy(service, identity) else {
        if let Some(reason) =
            observer::runtime::local_only_reason(&terminal.runtime.launch_privilege_facts())
        {
            return Err(failure(ErrorCode::AccessDenied, reason));
        }
        return unavailable(input.tab_id, m::UnavailableReason::FactsUnavailable);
    };
    let replay_only = terminal.runtime.source_kind() == shellfox_runtime::SourceKind::ExternalBytes;
    if replay_only && takeover {
        return Err(failure(ErrorCode::AccessDenied, "Automation replay is read-only."));
    }
    let view_id = connection.binding().connection_id;
    let view = state
        .manager_views
        .entries
        .get_mut(&view_id)
        .ok_or_else(|| failure(ErrorCode::AuthFailed, "Manager view missing."))?;
    let old = view
        .sources
        .values()
        .filter(|source| source.identity.tab_id == tab)
        .cloned()
        .collect::<Vec<_>>();
    for source in old {
        if let Some(current) = view.sources.get_mut(&source.delivery.delivery_id) {
            current.revoked = true;
        }
        retire(service, &source, m::RevokedReason::ViewDetached)?;
        view.sources.remove(&source.delivery.delivery_id);
        service.manager_v2_mailboxes.forget_notice(&source.delivery);
    }
    if !service.manager_v2_mailboxes.can_insert() {
        return unavailable(input.tab_id, m::UnavailableReason::FactsUnavailable);
    }
    let viewer = service.allocate_viewer_id().map_err(|_| native_error())?;
    let mut input_state = if replay_only {
        m::InputState::IntrinsicReadOnly
    } else {
        m::InputState::NotReady
    };
    let mut controller = None;
    if !replay_only && matches!(input.control, m::ControlRequest::IfFree) {
        if takeover {
            service.manager_v2_mailboxes.revoke_controllers(identity);
        }
        match service.attach_manager(tab, viewer, takeover) {
            Ok(offer) => {
                input_state = m::InputState::Owned;
                controller = Some(offer);
            }
            Err(error) => {
                if !takeover
                    && matches!(error.downcast_ref::<GrantFailure>(),Some(GrantFailure::Rejected(error))if error.code==shellfox_runtime::ErrorCode::ControlRequired)
                {
                    input_state = m::InputState::Busy;
                } else {
                    return Err(native_error());
                }
            }
        }
    }
    let offer = match controller {
        Some(offer) => Offer::Controller(offer),
        None => Offer::Observer(
            service
                .observer_grant(tab, viewer)
                .map_err(|_| native_error())?,
        ),
    };
    if offer.identity() != identity {
        return Err(failure(
            ErrorCode::TargetLost,
            "Source changed while granting delivery.",
        ));
    }
    let role = offer.role();
    let entry = match ManagerDelivery::new(identity, role) {
        Ok(entry) => entry,
        Err(_) => {
            // The unadopted actual offer remains in central Service custody.
            return unavailable(input.tab_id, m::UnavailableReason::RecoveryUnavailable);
        }
    };
    if service.manager_v2_mailboxes.insert_offer(entry).is_err() {
        return unavailable(input.tab_id, m::UnavailableReason::RecoveryUnavailable);
    }
    let selected = connection.selection();
    let command = match selected.recovery_format {
        m::RecoveryFormat::FormattedActiveBuffer => match role {
            Role::Controller(lease) => Command::RecoverStamped {
                lease,
                capability: shellfox_runtime::AnsiRecoveryCapability::FormattedActiveBufferV1,
            },
            Role::Observer(observer) => Command::ObserverRecoverStamped {
                observer,
                capability: shellfox_runtime::AnsiRecoveryCapability::FormattedActiveBufferV1,
            },
        },
        m::RecoveryFormat::Native0RawV1 => {
            {
                abandon(service, identity, role, m::RevokedReason::SourceUnavailable);
                return unavailable(input.tab_id, m::UnavailableReason::RecoveryUnavailable);
            }
        }
    };
    let recovery = service.call(tab, command).ok().and_then(|response| {
        service
            .manager_v2_mailboxes
            .get(identity, role)
            .and_then(|entry| match selected.recovery_format {
                m::RecoveryFormat::FormattedActiveBuffer => entry.recover(response).ok(),
                m::RecoveryFormat::Native0RawV1 => {
                    {
                        let _ = response;
                        None
                    }
                }
            })
    });
    let Some(recovery) = recovery else {
        abandon(service, identity, role, m::RevokedReason::SourceUnavailable);
        return unavailable(input.tab_id, m::UnavailableReason::RecoveryUnavailable);
    };
    if observer::runtime::effective_policy(service, identity) != Some(policy) {
        abandon(service, identity, role, m::RevokedReason::PolicyChanged);
        return unavailable(input.tab_id, m::UnavailableReason::FactsUnavailable);
    }
    let entry = service
        .manager_v2_mailboxes
        .get(identity, role)
        .ok_or_else(native_error)?;
    let source = Source {
        identity,
        role,
        delivery: entry.reference().clone(),
        control: entry.control().cloned(),
        layout: entry.layout().cloned(),
        policy,
        revoked: false,
    };
    let result = match checked(m::Attachment::Attached(Box::new(m::Attached {
        delivery: source.delivery.clone(),
        input: source.control.clone(),
        input_state,
        layout: source.layout.clone(),
        recovery,
    }))) {
        Ok(value) => value,
        Err(error) => {
            abandon(service, identity, role, m::RevokedReason::SourceUnavailable);
            return Err(error);
        }
    };
    // Bound the ACTUAL encoded reply including binding, refs and all metadata,
    // before adopting the grant. Never trim/split the recovery to fit a carrier.
    let encoded = serde_json::to_vec(&m::BoundReply {
        binding: connection.binding(),
        outcome: m::Reply::Success(&result),
    });
    if !encoded.is_ok_and(|bytes| bytes.len() <= wire::MAX_RESPONSE_BYTES) {
        abandon(service, identity, role, m::RevokedReason::SourceUnavailable);
        return Err(failure(
            ErrorCode::Validation,
            "Recovery reply exceeds the bounded carrier.",
        ));
    }
    // The outer OwnerRequest holds approval/current-connection fences. Check
    // canonical binding again without recursively locking that held fence.
    let binding = connection.binding();
    let valid = state
        .manager_views
        .entries
        .get(&view_id)
        .is_some_and(|view| view.connection.binding() == binding)
        && shellfox_protocol::AuthenticatedPeerId::new(connection.actor().peer_id.clone())
            .ok()
            .is_some_and(|peer| connection.authenticated().check(&peer, &binding).is_ok());
    if !valid {
        abandon(service, identity, role, m::RevokedReason::ViewDetached);
        return Err(failure(
            ErrorCode::AuthFailed,
            "Manager binding changed before adoption.",
        ));
    }
    let delivery_id = source.delivery.delivery_id;
    state
        .manager_views
        .entries
        .get_mut(&view_id)
        .ok_or_else(native_error)?
        .sources
        .insert(delivery_id, source);
    if offer.adopt(service).is_err() {
        state
            .manager_views
            .entries
            .get_mut(&view_id)
            .ok_or_else(native_error)?
            .sources
            .remove(&delivery_id);
        abandon(service, identity, role, m::RevokedReason::ViewDetached);
        return Err(native_error());
    }
    Ok(result)
}
fn abandon(
    service: &mut Service,
    identity: shellfox_runtime::Identity,
    role: Role,
    reason: m::RevokedReason,
) {
    let reference = service
        .manager_v2_mailboxes
        .get(identity, role)
        .map(|entry| entry.reference().clone());
    service.manager_v2_mailboxes.revoke(identity, role, reason);
    if let Some(reference) = reference {
        service.manager_v2_mailboxes.forget_notice(&reference);
    }
    // Only provisional bookkeeping is removed. Offer drop owns retirement.
    let _ = service
        .manager_v2_mailboxes
        .discard_unadopted(identity, role);
}
fn unavailable(tab_id: m::Id, reason: m::UnavailableReason) -> Result<serde_json::Value, AppError> {
    checked(m::Attachment::Unavailable { tab_id, reason })
}
