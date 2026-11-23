use super::*;
use shellfox_core::{CreateSession, SessionId};
use shellfox_runtime::{Command, ErrorCode, Lease, RuntimeError};

/// The runtime's answer to a readiness check: a current controller lease that
/// hasn't consumed its recovery yet gets RecoveryRequired; a stale one doesn't.
fn readiness(service: &mut Service, tab: TabId, lease: Lease) -> Option<ErrorCode> {
    let error = service
        .call(tab, Command::CheckControllerReady { lease })
        .err()?;
    Some(
        error
            .downcast_ref::<RuntimeError>()
            .map_or(ErrorCode::Unavailable, |error| error.code),
    )
}

#[test]
#[ignore = "explicit owned raw fixture only; set SHELLFOX_RAW_TEST_HELPER"]
fn manager_if_free_does_not_take_a_runtime_owner_after_service_slot_detach() {
    let (scratch, mut owned, _) = service();
    let service = &mut owned.0;
    let snapshot = service
        .create_application_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("owned manager attach policy".into()),
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: crate::tests::default_catalog_profile(),
        })
        .unwrap();
    let tab = snapshot.sessions[0].tabs[0].id;
    service.attach(tab, 11, false).unwrap();
    service.detach_controller(11);
    let other_offer = service.controller_grant(tab, 22, true).unwrap();
    let other = other_offer.lease();
    assert!(service.attach_manager(tab, 33, false).is_err());
    let current = Some(ErrorCode::RecoveryRequired);
    assert_eq!(
        readiness(service, tab, other),
        current,
        "the manager took nothing"
    );
    let taken = service.attach_manager(tab, 33, true).unwrap();
    assert_eq!(taken.lease().controller, 33);
    assert!(taken.lease().epoch > other.epoch);
    let stale = readiness(service, tab, other);
    assert!(
        stale.is_some() && stale != current,
        "the takeover retired the old lease"
    );
    assert_eq!(readiness(service, tab, taken.lease()), current);
    // Both actual offers remain in central custody; no fake M2 mailbox/adoption.
    service.close_tab(tab).unwrap();
    service.shutdown().unwrap();
}
