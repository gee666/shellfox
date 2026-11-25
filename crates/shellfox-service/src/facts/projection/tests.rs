use super::*;
use crate::Service;
use shellfox_core::{CreateSession, ProfileId, SessionId};

fn fixture() -> (tempfile::TempDir, Service) {
    let data = crate::tests::scratch();
    let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    (data, service)
}
#[test]
fn actual_app_creation_projects_without_invented_runtime() {
    let (data, mut service) = fixture();
    // The application creation itself, before its shell starts.
    let snapshot = service
        .create_application_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: crate::tests::default_catalog_profile(),
        })
        .unwrap();
    service.pump_activity();
    let source = crate::activity::integration::read_source(&service.source_facts_watch()).unwrap();
    let result = project_session_facts(&source).unwrap();
    let id = snapshot.selection.session_id.unwrap();
    let session = &result[&id];
    let expected_shell = shell(source.creation_facts[&id].shell_id);
    assert_eq!(
        std::mem::discriminant(&session.shell_id),
        std::mem::discriminant(&expected_shell)
    );
    let tab = &snapshot.sessions[0].tabs[0];
    assert_eq!(
        session.tabs[&tab.id].created_at,
        tab.created_at.as_ref().unwrap().as_str()
    );
    assert!(matches!(
        session.tabs[&tab.id].observation,
        shellfox_remote::adapter::ObservationFacts::Unavailable { .. }
    ));
    assert!(session.tabs[&tab.id].generation.is_none());
}
#[test]
fn unknown_creation_time_or_session_intent_is_unavailable_without_backfill() {
    let (data, mut service) = fixture();
    service
        .host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    service.pump_activity();
    let source = crate::activity::integration::read_source(&service.source_facts_watch()).unwrap();
    assert!(project_session_facts(&source).is_err());
    let (data, mut service) = fixture();
    service
        .create_application_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: crate::tests::default_catalog_profile(),
        })
        .unwrap();
    service.pump_activity();
    let source = crate::activity::integration::read_source(&service.source_facts_watch()).unwrap();
    let mut missing = (*source).clone();
    missing.snapshot.sessions[0].tabs[0].created_at = None;
    assert!(project_session_facts(&missing).is_err());
}
