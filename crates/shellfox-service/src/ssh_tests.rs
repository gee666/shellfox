use super::*;
#[path = "ssh_test_support.rs"]
mod support;
use support::*;
#[path = "ssh_broker_tests.rs"]
mod broker_tests;
#[path = "ssh_entry_tests.rs"]
mod entry_tests;
#[path = "ssh_lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "ssh_native_tests.rs"]
mod native_tests;
#[path = "ssh_reservation_tests.rs"]
mod reservation_tests;
#[path = "ssh_selection_tests.rs"]
mod selection_tests;
#[test]
fn native_crud_preserves_password_tristate_and_rejects_revoked_mutations() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let mut p = input();
    let id = p.id;
    manager
        .request(7, Request::Save(p.clone()), &validator, &mut starter)
        .unwrap();
    p.password = Password::Keep;
    manager
        .request(7, Request::Save(p.clone()), &validator, &mut starter)
        .unwrap();
    assert!(manager
        .profiles
        .get_for_connection(id)
        .unwrap()
        .unwrap()
        .password
        .is_some());
    p.password = Password::Clear;
    manager
        .request(7, Request::Save(p), &validator, &mut starter)
        .unwrap();
    assert!(manager
        .profiles
        .get_for_connection(id)
        .unwrap()
        .unwrap()
        .password
        .is_none());
    validator.0.store(false, Ordering::SeqCst);
    assert!(manager
        .request(7, Request::Delete { id }, &validator, &mut starter)
        .is_err());
    assert_eq!(manager.profiles.list().unwrap().len(), 1);
}
#[test]
fn trust_and_readiness_are_not_spawn_and_cleanup_retains_the_same_generation() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &validator, &mut starter);
    approve(&mut manager, &validator, &mut starter, id);
    drive(&mut manager, &validator, &mut starter, |m| {
        matches!(m.connections[&id].status.state, State::Authenticating)
    });
    assert_eq!(starter.starts, 1);
    let identity = starter.identity.unwrap();
    let target = manager.source_target(identity.tab_id).unwrap();
    manager.observe_output(identity, b"PTY Started authentication-looking output");
    assert_eq!(manager.connections[&id].status.state, State::Authenticating);
    manager
        .request(
            7,
            Request::Disconnect { connection: id },
            &validator,
            &mut starter,
        )
        .unwrap();
    assert_eq!(manager.connections[&id].runtime, Some(identity));
    assert!(manager.connections[&id].broker.is_some());
    assert_eq!(manager.source_target(identity.tab_id), Some(target));
    assert!(!manager.is_closed());
    assert_eq!(starter.releases, 0);
    starter.allow_close = true;
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
    assert_eq!(manager.connections[&id].status.state, State::Closed);
    assert_eq!(starter.releases, 1);
}
#[test]
fn closed_before_proof_reports_failure_instead_of_ready() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &validator, &mut starter);
    approve(&mut manager, &validator, &mut starter, id);
    drive(&mut manager, &validator, &mut starter, |m| {
        matches!(m.connections[&id].status.state, State::Authenticating)
    });
    manager.runtime_closed(starter.identity.take().unwrap());
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
    assert!(matches!(
        manager.connections[&id].status.state,
        State::Failed { .. }
    ));
}
#[test]
fn revoked_controller_cannot_answer_stale_host_challenge_or_start_runtime() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &validator, &mut starter);
    drive(&mut manager, &validator, &mut starter, |m| {
        matches!(m.connections[&id].status.state, State::AwaitingHost(_))
    });
    validator.0.store(false, Ordering::SeqCst);
    manager.poll(&validator, &mut starter);
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
    assert_eq!(starter.starts, 0);
    assert_eq!(starter.releases, 1);
    assert!(!Path::new(fixture.directory.path())
        .join("ssh-known-hosts.json")
        .exists());
}
