use super::*;
use shellfox_core::{Generation, SessionId, TabId};
use std::time::Instant;
fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn failed(result: Result<Runtime, ExternalStartError>) -> ExternalStartError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("controlled startup must fail"),
    }
}
fn wait_closed(owner: &mut ExternalStartupOwner) {
    owner.request_cancel();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !owner.poll_closed() {
        assert!(
            Instant::now() < deadline,
            "retained parser owner did not close"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn delayed_ready() {
    std::thread::sleep(Duration::from_millis(100));
}
fn lost_ready_channel() {
    panic!("controlled external readiness-channel failure");
}
#[test]
fn invalid_config_is_before_thread_and_has_no_retained_parser_owner() {
    let error = failed(start(
        identity(),
        ExternalConfig {
            cols: 0,
            ..Default::default()
        },
        StartupOptions::default(),
    ));
    assert_eq!(error.stage, ExternalStartStage::BeforeThread);
    assert!(error.take_owner().is_none());
}
#[test]
fn timeout_returns_original_cancellable_owner_until_join_proves_it_closed() {
    let identity = identity();
    let error = failed(start(
        identity,
        ExternalConfig::default(),
        StartupOptions {
            wait: Duration::ZERO,
            before_ready: Some(delayed_ready),
            ..Default::default()
        },
    ));
    assert_eq!(error.stage, ExternalStartStage::OwnerUnconfirmed);
    let mut owner = error
        .take_owner()
        .expect("uncertain startup must retain its controller");
    assert!(error.take_owner().is_none());
    assert_eq!(owner.identity(), identity);
    wait_closed(&mut owner);
    assert!(owner.poll_closed());
}
#[test]
fn readiness_channel_failure_retains_owner_even_if_phase_is_not_closed() {
    let error = failed(start(
        identity(),
        ExternalConfig::default(),
        StartupOptions {
            wait: Duration::from_secs(1),
            before_ready: Some(lost_ready_channel),
            ..Default::default()
        },
    ));
    assert_eq!(error.stage, ExternalStartStage::OwnerUnconfirmed);
    let mut owner = error.take_owner().expect("channel loss must retain owner");
    wait_closed(&mut owner);
}
#[test]
fn startup_error_can_be_retained_by_a_send_sync_service_error() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<ExternalStartError>();
}
