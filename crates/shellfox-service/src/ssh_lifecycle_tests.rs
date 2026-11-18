use super::*;
#[test]
fn failed_stop_keeps_label_while_draining_held_broker_runtime_and_reservation() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &validator, &mut starter);
    approve(&mut manager, &validator, &mut starter, id);
    drive(&mut manager, &validator, &mut starter, |m| {
        m.connections[&id].runtime.is_some()
    });
    let identity = starter.identity.unwrap();
    assert!(manager.connections[&id].broker.is_some());
    let failed = State::Failed {
        message: "synthetic held-resource failure".into(),
    };
    manager.connections.get_mut(&id).unwrap().status.state = failed.clone();
    for _ in 0..5 {
        manager.begin_shutdown();
        manager.poll(&validator, &mut starter);
        assert_eq!(manager.connections[&id].status.state, failed);
        assert_eq!(manager.connections[&id].runtime, Some(identity));
        assert!(manager.connections[&id].broker.is_some());
        assert!(manager.source_target(identity.tab_id).is_some());
        assert!(!manager.is_closed());
        assert_eq!(starter.releases, 0);
    }
    starter.allow_close = true;
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
    assert_eq!(manager.connections[&id].status.state, failed);
    assert!(manager.connections[&id].broker.is_none());
    assert!(manager.connections[&id].job.is_none());
    assert!(manager.connections[&id].identity.is_none());
    assert!(manager.source_target(identity.tab_id).is_none());
    assert_eq!(starter.releases, 1);
}
#[test]
fn ready_survives_view_detach_takeover_and_checked_adoption_without_a_claim() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let original_view = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &original_view, &mut starter);
    let identity = authenticate(&mut manager, &original_view, &mut starter, id);
    let target = manager.source_target(identity.tab_id).unwrap();
    manager.detach_controller(7);
    original_view.0.store(false, Ordering::SeqCst);
    manager.poll(&original_view, &mut starter);
    assert_eq!(manager.connections[&id].status.state, State::Ready);
    assert_eq!(manager.connections[&id].runtime, Some(identity));
    assert_eq!(starter.closes, 0);
    assert_eq!(starter.starts, 1);
    assert_eq!(manager.source_target(identity.tab_id), Some(target));
    let new_view = Validator(AtomicBool::new(true));
    let Response::Status(rows) = manager
        .request(
            8,
            Request::GetStatus { connection: None },
            &new_view,
            &mut starter,
        )
        .unwrap()
    else {
        panic!("status")
    };
    assert!(rows
        .iter()
        .any(|r| r.connection == id && r.state == State::Ready));
    manager
        .request(
            8,
            Request::AdoptReady { connection: id },
            &new_view,
            &mut starter,
        )
        .unwrap();
    assert_eq!(manager.connections[&id].controller, 8);
    assert!(manager.connections[&id].view_attached);
    assert_eq!(starter.claims, 0);
    assert_eq!(starter.starts, 1);
    assert_eq!(starter.closes, 0);
    assert_eq!(starter.identity, Some(identity));
    manager.detach_controller(7);
    assert!(manager
        .request(
            7,
            Request::Disconnect { connection: id },
            &new_view,
            &mut starter
        )
        .is_err());
    assert_eq!(manager.connections[&id].status.state, State::Ready);
    manager.revoke_native_capability(8);
    manager.poll(&new_view, &mut starter);
    assert_eq!(manager.connections[&id].runtime, Some(identity));
    assert!(manager.connections[&id].broker.is_some());
    assert_eq!(manager.source_target(identity.tab_id), Some(target));
    starter.allow_close = true;
    drive(&mut manager, &new_view, &mut starter, SshManager::is_closed);
    assert_eq!(starter.releases, 1);
    assert!(manager.source_target(identity.tab_id).is_none());
}
#[test]
fn detaching_pending_auth_retains_runtime_broker_and_reservation_until_confirmed_close() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &validator, &mut starter);
    approve(&mut manager, &validator, &mut starter, id);
    drive(&mut manager, &validator, &mut starter, |m| {
        matches!(m.connections[&id].status.state, State::Authenticating)
    });
    let identity = starter.identity.unwrap();
    manager.detach_controller(7);
    manager.poll(&validator, &mut starter);
    assert_eq!(manager.connections[&id].status.state, State::Closing);
    assert_eq!(manager.connections[&id].runtime, Some(identity));
    assert!(manager.connections[&id].broker.is_some());
    assert!(manager.source_target(identity.tab_id).is_some());
    assert_eq!(starter.releases, 0);
    let command = starter.command.as_ref().unwrap();
    let marker = command
        .split("printf '\\033")
        .nth(1)
        .and_then(|s| s.split("\\007'").next())
        .unwrap();
    manager.observe_output(identity, format!("\x1b{marker}\x07").as_bytes());
    assert_ne!(manager.connections[&id].status.state, State::Ready);
    starter.allow_close = true;
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
    assert_eq!(starter.releases, 1);
}
#[test]
fn ready_view_detach_cancels_prompt_but_not_runtime_or_source() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &validator, &mut starter);
    let identity = authenticate(&mut manager, &validator, &mut starter, id);
    let (reply, answer) = std::sync::mpsc::sync_channel(1);
    manager
        .events
        .try_send(WorkEvent::Prompt {
            connection: id,
            value: Prompt {
                prompt: Uuid::new_v4(),
                label: "Synthetic prompt".into(),
                echo: false,
            },
            reply,
        })
        .unwrap_or_else(|_| panic!("event queue"));
    manager.poll(&validator, &mut starter);
    assert!(matches!(
        manager.connections[&id].status.state,
        State::AwaitingPrompt(_)
    ));
    manager.observe_output(
        identity,
        b"ordinary output must not hide the pending prompt",
    );
    assert!(matches!(
        manager.connections[&id].status.state,
        State::AwaitingPrompt(_)
    ));
    manager.detach_controller(7);
    assert!(answer.recv_timeout(Duration::from_secs(1)).is_err());
    manager.poll(&validator, &mut starter);
    assert_eq!(manager.connections[&id].status.state, State::Ready);
    assert_eq!(manager.connections[&id].runtime, Some(identity));
    assert_eq!(starter.closes, 0);
    assert!(manager.source_target(identity.tab_id).is_some());
    manager.begin_shutdown();
    starter.allow_close = true;
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
}
struct DeniedTarget;
impl NativeControllerValidator for DeniedTarget {
    fn validate_native(&self, _: u64) -> Result<()> {
        Ok(())
    }
    fn validate_target(&self, _: u64, _: shellfox_core::TabId) -> Result<()> {
        anyhow::bail!("Tab application permission denied.")
    }
}
#[test]
fn adoption_rechecks_native_target_and_generation_and_app_stop_closes_ready() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let validator = Validator(AtomicBool::new(true));
    let mut starter = Starter::default();
    let id = connect(&mut manager, &validator, &mut starter);
    let identity = authenticate(&mut manager, &validator, &mut starter, id);
    assert!(manager.adopt_ready(8, id, &DeniedTarget, &starter).is_err());
    let absent = starter.identity.take();
    assert!(manager.adopt_ready(8, id, &validator, &starter).is_err());
    starter.identity = absent;
    assert!(manager.adopt_ready(99, id, &validator, &starter).is_err());
    assert_eq!(starter.claims, 0);
    assert_eq!(manager.connections[&id].controller, 7);
    manager.detach_controller(7);
    validator.0.store(false, Ordering::SeqCst);
    manager.poll(&validator, &mut starter);
    assert_eq!(manager.connections[&id].runtime, Some(identity));
    assert_eq!(starter.closes, 0);
    manager.begin_shutdown();
    starter.allow_close = true;
    drive(
        &mut manager,
        &validator,
        &mut starter,
        SshManager::is_closed,
    );
    assert!(starter.identity.is_none());
}
