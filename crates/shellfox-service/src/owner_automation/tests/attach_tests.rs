use super::*;
use shellfox_core::{CreateSession, ProfileId, SessionId};
use shellfox_runtime::{Command, Response};
#[test]
#[ignore = "explicit owned Windows supervisor; no unrelated process or network"]
fn manager_if_free_does_not_take_a_runtime_owner_after_service_slot_detach() {
    let (scratch, mut owned, _) = service();
    let service = &mut owned.0;
    let snapshot = service
        .create_application_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("owned manager attach policy".into()),
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let tab = snapshot.sessions[0].tabs[0].id;
    service.attach(tab, 11, false).unwrap();
    service.detach_controller(11);
    let other_offer = service.controller_grant(tab, 22, true).unwrap();
    let other = other_offer.lease();
    assert!(service.attach_manager(tab, 33, false).is_err());
    assert!(matches!(
        service
            .call(tab, Command::CheckControllerReady { lease: other })
            .unwrap(),
        Response::Applied
    ));
    let taken = service.attach_manager(tab, 33, true).unwrap();
    assert_eq!(taken.lease().controller, 33);
    assert!(taken.lease().epoch > other.epoch);
    assert!(service
        .call(tab, Command::CheckControllerReady { lease: other })
        .is_err());
    assert!(matches!(
        service
            .call(
                tab,
                Command::CheckControllerReady {
                    lease: taken.lease()
                }
            )
            .unwrap(),
        Response::Applied
    ));
    // Both actual offers remain in central custody; no fake M2 mailbox/adoption.
    service.close_tab(tab).unwrap();
    service.shutdown().unwrap();
}
