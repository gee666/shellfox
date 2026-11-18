use crate::{experiment::ExperimentRequest, native::NativeAction, tests::scratch, Service};
use shellfox_core::{CreateSession, ProfileId, SessionId};

#[test]
fn stop_fences_queued_resource_mutations_before_cleanup_and_never_reopens() {
    let data = scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let snapshot = service
        .host()
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("stop fence".into()),
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let session = snapshot.sessions[0].id;
    let tab = snapshot.sessions[0].tabs[0].id;
    let before = service.host().snapshot().unwrap();
    service.execute(ExperimentRequest::Stop {}, 1).unwrap();
    assert!(service.stopping);
    assert!(service.start_terminal(tab).is_err());
    assert!(service
        .execute(
            ExperimentRequest::Attach {
                tab,
                takeover: false
            },
            1
        )
        .is_err());
    assert!(service
        .execute_native(
            NativeAction::Attach {
                tab,
                takeover: false
            },
            2
        )
        .is_err());
    assert!(service
        .execute(
            ExperimentRequest::RenameSession {
                id: session,
                title: "must not commit".into()
            },
            1
        )
        .is_err());
    assert_eq!(service.active_terminals(), 0);
    assert_eq!(service.host().snapshot().unwrap(), before);
    // Cleanup and final reads remain possible; retry never resets admission.
    service.execute(ExperimentRequest::Snapshot {}, 1).unwrap();
    service.shutdown().unwrap();
    assert!(service.stopping);
}
