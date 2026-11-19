use super::*;
use shellfox_runtime::{
    Identity, LaunchEvidenceBasis, LaunchEvidenceStage, LaunchFactoryPurpose, LaunchPrivilege,
    LaunchPrivilegeFacts, LaunchUnknownReason, ObserverLease,
};
#[derive(Clone)]
pub(super) struct Attached {
    pub identity: Identity,
    pub observer: ObserverLease,
    pub reference: v2::DeliveryRef,
    pub policy_epoch: u64,
    pub revoked: bool,
}
fn original_program_normal(facts: &LaunchPrivilegeFacts) -> bool {
    facts.privilege == LaunchPrivilege::Normal
        && facts.purpose == LaunchFactoryPurpose::OwnedPtyProgram
        && facts.stage == LaunchEvidenceStage::BeforePublicPtyReady
}
#[cfg(any(target_os = "linux", test))]
fn exact_posix_normal(facts: &LaunchPrivilegeFacts, owner: u32) -> bool {
    original_program_normal(facts)
        && facts.basis == LaunchEvidenceBasis::OriginalPinnedPosixCredentials
        && facts.token.is_none()
        && facts.unknown.is_none()
        && facts.posix.is_some_and(|p| p.is_non_elevating_account(owner))
}
#[cfg(any(target_os = "macos", test))]
fn exact_darwin_normal(facts: &LaunchPrivilegeFacts, owner: u32) -> bool {
    original_program_normal(facts)
        && owner != 0
        && facts.basis == (LaunchEvidenceBasis::OriginalDarwinCredentials { uid: owner })
        && facts.token.is_none()
        && facts.posix.is_none()
        && facts.unknown.is_none()
}
fn accepted_launch(facts: &LaunchPrivilegeFacts) -> bool {
    if local_only_reason(facts).is_some() || !original_program_normal(facts) {
        return false;
    }
    match facts.basis {
        // Existing Windows semantics remain the same original-token gate.
        LaunchEvidenceBasis::OriginalPinnedChildToken => cfg!(windows),
        LaunchEvidenceBasis::OriginalPinnedPosixCredentials => {
            #[cfg(target_os = "linux")]
            {
                let owner = rustix::process::getuid().as_raw();
                owner == rustix::process::geteuid().as_raw() && exact_posix_normal(facts, owner)
            }
            #[cfg(not(target_os = "linux"))]
            {
                false
            }
        }
        #[cfg(target_os = "macos")]
        LaunchEvidenceBasis::OriginalDarwinCredentials { .. } => {
            let owner = rustix::process::getuid().as_raw();
            owner == rustix::process::geteuid().as_raw() && exact_darwin_normal(facts, owner)
        }
        _ => false,
    }
}
pub(crate) fn local_only_reason(facts: &LaunchPrivilegeFacts) -> Option<&'static str> {
    if facts.privilege == LaunchPrivilege::Elevated {
        Some("Elevated sources remain local-only.")
    } else if facts.unknown == Some(LaunchUnknownReason::UnrestrictedPosixLaunch) {
        Some("This source is local-only: it was launched with local elevation available. Create a new explicitly non-elevating remote-eligible source.")
    } else {
        None
    }
}
#[cfg(test)]
#[path = "remote_v2_privilege_tests.rs"]
mod privilege_tests;
/// Eligibility is native owner policy, privilege is sealed initial-launch fact.
/// Neither alone grants delivery. Metadata/device/connection checks remain apart.
pub(in crate::remote) fn effective_policy(service: &Service, identity: Identity) -> Option<u64> {
    let epoch = service.observer_delivery_policy(identity)?;
    let terminal = service
        .terminals
        .get(&identity.tab_id)
        .filter(|terminal| terminal.runtime.identity() == identity)?;
    if !matches!(terminal.startup, Some(Ok(()))) {
        return None;
    }
    // Passive automation eligibility already requires its distinct sealed raw
    // proof and a published, complete run. PTYs retain their own launch gate.
    (terminal.runtime.source_kind() == shellfox_runtime::SourceKind::ExternalBytes
        || accepted_launch(&terminal.runtime.launch_privilege_facts())).then_some(epoch)
}
pub(super) fn acknowledge(
    service: &mut Service,
    view: &mut ObserverView,
    input: v2::AcknowledgeTerminal,
) -> Result<serde_json::Value, AppError> {
    let attached = view
        .deliveries
        .get_mut(&input.delivery.delivery_id)
        .filter(|attached| attached.reference == input.delivery)
        .ok_or_else(|| {
            failure(
                ErrorCode::AuthFailed,
                "Delivery does not belong to this connection.",
            )
        })?;
    if attached.revoked
        || effective_policy(service, attached.identity) != Some(attached.policy_epoch)
    {
        attached.revoked = true;
        let _ = service.retire_remote_observer(attached.identity, attached.observer);
        return Err(failure(
            ErrorCode::TargetLost,
            "Source policy or exact generation was revoked.",
        ));
    }
    let command = service
        .remote_v2_mailboxes
        .get_mut(attached.identity, attached.observer)
        .ok_or_else(|| {
            failure(
                ErrorCode::TargetLost,
                "Original observer source is unavailable.",
            )
        })?
        .ack_command(&input.delivery, &input.ack)
        .map_err(|_| {
            failure(
                ErrorCode::Validation,
                "Observer ACK is not an offered recovery/data boundary.",
            )
        })?;
    let response = service
        .call(attached.identity.tab_id, command)
        .map_err(|_| {
            failure(
                ErrorCode::NativeUnavailable,
                "Observer ACK outcome is uncertain; no input was replayed.",
            )
        })?;
    if effective_policy(service, attached.identity) != Some(attached.policy_epoch) {
        attached.revoked = true;
        let _ = service.retire_remote_observer(attached.identity, attached.observer);
        return Err(failure(
            ErrorCode::TargetLost,
            "Source policy changed during ACK.",
        ));
    }
    service
        .remote_v2_mailboxes
        .get_mut(attached.identity, attached.observer)
        .ok_or_else(|| failure(ErrorCode::TargetLost, "Original observer disappeared."))?
        .commit_ack(&input.delivery, &input.ack, &response)
        .map_err(|_| {
            failure(
                ErrorCode::TargetLost,
                "Observer recovery/layout changed during ACK.",
            )
        })?;
    checked(wire::Acknowledged { acknowledged: true })
}
pub(super) fn detach(
    service: &mut Service,
    view: &mut ObserverView,
    input: v2::DetachTerminal,
) -> Result<serde_json::Value, AppError> {
    let attached = view
        .deliveries
        .get_mut(&input.delivery.delivery_id)
        .filter(|attached| attached.reference == input.delivery)
        .ok_or_else(|| {
            failure(
                ErrorCode::AuthFailed,
                "Delivery does not belong to this connection.",
            )
        })?;
    attached.revoked = true;
    service
        .retire_remote_observer(attached.identity, attached.observer)
        .map_err(|_| {
            failure(
                ErrorCode::NativeUnavailable,
                "Observer retirement retained.",
            )
        })?;
    view.deliveries.remove(&input.delivery.delivery_id);
    checked(wire::Detached { detached: true })
}
pub(super) fn detach_all(service: &mut Service, view: &mut ObserverView) -> Result<(), AppError> {
    for attached in view.deliveries.values_mut() {
        attached.revoked = true;
        service
            .retire_remote_observer(attached.identity, attached.observer)
            .map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Observer retirement retained.",
                )
            })?;
    }
    view.deliveries.clear();
    Ok(())
}
pub(super) fn poll(
    service: &mut Service,
    view: &mut ObserverView,
    snapshot: &Snapshot,
    allowed: &std::collections::HashSet<shellfox_core::TabId>,
    events: &mut Vec<v2::HostEvent>,
) -> Result<(), AppError> {
    let mut budget = v2::MAX_POLL_DATA_BYTES;
    for attached in view.deliveries.values_mut() {
        if attached.revoked
            || !allowed.contains(&attached.identity.tab_id)
            || !crate::facts_membership::contains(snapshot, attached.identity)
            || effective_policy(service, attached.identity) != Some(attached.policy_epoch)
        {
            attached.revoked = true;
            let _ = service.retire_remote_observer(attached.identity, attached.observer);
            events.push(v2::HostEvent::Terminal {
                event: v2::Event::Revoked {
                    delivery: attached.reference.clone(),
                    reason: v2::RevokedReason::PolicyChanged,
                },
            });
            continue;
        }
        service
            .drain(attached.identity.tab_id)
            .map_err(|_| failure(ErrorCode::TargetLost, "Observer source was replaced."))?;
        if effective_policy(service, attached.identity) != Some(attached.policy_epoch) {
            attached.revoked = true;
            let _ = service.retire_remote_observer(attached.identity, attached.observer);
            events.push(v2::HostEvent::Terminal {
                event: v2::Event::Revoked {
                    delivery: attached.reference.clone(),
                    reason: v2::RevokedReason::PolicyChanged,
                },
            });
            continue;
        }
        let entry = service
            .remote_v2_mailboxes
            .get_mut(attached.identity, attached.observer)
            .ok_or_else(|| failure(ErrorCode::TargetLost, "Observer view is unavailable."))?;
        let offered = entry
            .poll(budget, 1024usize.saturating_sub(events.len()))
            .map_err(|_| {
                failure(
                    ErrorCode::TargetLost,
                    "Observer replay was lost; request full recovery explicitly.",
                )
            })?;
        for event in offered {
            budget = budget.saturating_sub(
                event
                    .decoded_bytes()
                    .map_err(|_| failure(ErrorCode::Internal, "Invalid observer data."))?,
            );
            events.push(v2::HostEvent::Terminal { event });
        }
    }
    Ok(())
}
