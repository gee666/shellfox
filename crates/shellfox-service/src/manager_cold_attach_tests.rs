//! Owner factory tests, not remote Go/SQL approval acceptance.
use super::*;
use shellfox_core::{CreateSession, ProfileId, SessionId};
fn profile() -> ProfileId {
    ProfileId::new(if cfg!(windows) { "cmd" } else { "login-shell" }).unwrap()
}
fn create(service: &Service, root: &std::path::Path, recorded: bool) -> shellfox_core::Session {
    let input = CreateSession {
        request_id: SessionId::new(),
        title: Some("cold source fixture".into()),
        cwd: root.to_string_lossy().into_owned(),
        profile_id: profile(),
    };
    let request = input.request_id;
    let snapshot = if recorded {
        service.create_application_session(input).unwrap()
    } else {
        service.host().create_session(input).unwrap()
    };
    let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } =
        service.host().creation_request_receipt(request).unwrap()
    else {
        panic!("original creation receipt")
    };
    snapshot
        .sessions
        .into_iter()
        .find(|session| session.id == session_id)
        .unwrap()
}
#[test]
fn observer_takeover_and_missing_old_resume_do_not_admit_new_source() {
    let mut input = m::AttachTerminal {
        tab_id: m::Id::new(),
        resume: None,
        control: m::ControlRequest::None,
    };
    assert!(!new_source_intent(
        &input,
        false,
        m::RecoveryFormat::Native0RawV1
    ));
    input.control = m::ControlRequest::IfFree;
    assert!(!new_source_intent(
        &input,
        true,
        m::RecoveryFormat::Native0RawV1
    ));
    assert!(!new_source_intent(
        &input,
        false,
        m::RecoveryFormat::FormattedActiveBuffer
    ));
    input.resume = Some(m::Resume {
        generation: m::Id::new(),
        after_sequence: 0,
    });
    assert!(!new_source_intent(
        &input,
        false,
        m::RecoveryFormat::Native0RawV1
    ));
}
#[test]
fn unknown_birth_adapter_and_archived_metadata_never_become_cold_admission() {
    let data = crate::tests::scratch();
    let service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let unknown = create(&service, data.path(), false);
    assert!(!recorded_target(&service, &unknown, unknown.tabs[0].id).unwrap());
    let known = create(&service, data.path(), true);
    let tab = known.tabs[0].id;
    assert!(recorded_target(&service, &known, tab).unwrap());
    let mut missing = known.clone();
    missing.tabs[0].created_at = None;
    assert!(!recorded_target(&service, &missing, tab).unwrap());
    let mut archived = known;
    archived.tabs[0].state = shellfox_core::TabState::Archived;
    assert!(!recorded_target(&service, &archived, tab).unwrap());
    assert!(service.terminals.is_empty());
}
#[test]
fn disabled_or_non_native_factory_has_no_generation_activity_or_registration_effect() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let session = create(&service, data.path(), true);
    let tab = session.tabs[0].id;
    let before = std::fs::read_dir(&service.isolation).unwrap().count();
    assert!(!service.start_manager_native0_terminal(tab).unwrap());
    service
        .install_observer_factory_policy(crate::NativeFactoryPolicy::ExistingManagerStandardCatalog)
        .unwrap();
    assert!(!service.start_manager_native0_terminal(tab).unwrap());
    assert!(service.terminals.is_empty());
    assert_eq!(
        std::fs::read_dir(&service.isolation).unwrap().count(),
        before
    );
    let close = service.host().prepare_close_tab(tab).unwrap();
    assert!(close.terminals.is_empty());
    service.host().cancel_close(close.token).unwrap();
}
