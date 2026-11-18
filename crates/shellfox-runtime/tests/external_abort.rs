#[path = "support/external.rs"]
mod common;
use common::*;
use shellfox_runtime::*;
use std::time::{Duration, Instant};
fn failed(runtime: &Runtime) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !runtime.external_status().unwrap().failed {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn joined(runtime: &mut Runtime) {
    runtime.request_external_close().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !runtime.poll_external_closed().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn producer_abort_disconnects_actual_sender_preserves_identity_and_drains_accepted_bytes() {
    let (mut runtime, mut source) = source();
    assert_eq!(
        request(&runtime, 1, Command::RetryClose).unwrap_err().code,
        ErrorCode::Unsupported
    );
    feed(
        &mut source,
        1,
        ExternalStream::Stdout,
        b"ACCEPTED_BEFORE_ABORT".to_vec(),
    );
    let evidence = source.abort(ExternalAbortReason::RawStartupFailed);
    assert_eq!(evidence.identity, runtime.identity());
    assert_eq!(evidence.last_queued, SourceSequence(1));
    failed(&runtime); // actual Receiver::Disconnected, not a rewritten status flag
    let status = runtime.external_status().unwrap();
    assert_eq!(status.abort, Some(evidence));
    assert_eq!(status.source_sequence, SourceSequence(1));
    assert!(!status.finished && !status.closed && status.finish_delivery.is_none());
    let observer = observe(&runtime, 2, 51, None);
    assert!(text(&frame(&runtime, 3, observer)).contains("ACCEPTED_BEFORE_ABORT"));
    // No raw process exists in this generic fixture. Production must separately
    // retain/prove cleanup of its raw startup owner before this retirement step.
    joined(&mut runtime);
    assert!(runtime.external_status().unwrap().closed);
}
#[test]
fn explicit_empty_finish_is_distinct_from_producer_failure() {
    let (mut runtime, mut source) = source();
    finish(&mut source, 0); // both streams empty/EOF in this fixed generic fixture
    wait(&runtime, || runtime.external_status().unwrap().finished);
    let status = runtime.external_status().unwrap();
    assert!(!status.failed && status.abort.is_none() && !status.closed);
    joined(&mut runtime);
}
#[test]
fn dropping_finished_producer_with_abort_intent_never_relabels_success_as_failure() {
    let (mut runtime, mut source) = source();
    feed(&mut source, 1, ExternalStream::Stdout, b"success".to_vec());
    finish(&mut source, 1);
    wait(&runtime, || runtime.external_status().unwrap().finished);
    let evidence = source.abort(ExternalAbortReason::ProducerFailed);
    let status = runtime.external_status().unwrap();
    assert_eq!(status.abort, Some(evidence)); // request intent only, not failure proof
    assert!(status.finished && !status.failed && !status.closed);
    let observer = observe(&runtime, 1, 53, None);
    assert!(text(&frame(&runtime, 2, observer)).contains("success"));
    assert!(!runtime.external_status().unwrap().failed);
    joined(&mut runtime);
}
#[test]
fn producer_abort_does_not_fake_pipe_eof_or_flush_incomplete_legacy_utf8() {
    let (mut runtime, mut source) = source_with_policy(ExternalDisplayPolicy::LegacyAutomationV1);
    feed(&mut source, 1, ExternalStream::Stdout, vec![0xf0, 0x9f]);
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    assert!(runtime.external_status().unwrap().delivery.is_none());
    let evidence = source.abort(ExternalAbortReason::CancelledBeforeRawLaunch);
    failed(&runtime);
    let status = runtime.external_status().unwrap();
    assert_eq!(status.abort, Some(evidence));
    assert_eq!(status.last_delivery_sequence, DeliverySequence(0));
    assert!(status.finish_delivery.is_none() && !status.finished);
    let observer = observe(&runtime, 1, 52, None);
    assert!(!text(&frame(&runtime, 2, observer)).contains('�'));
    joined(&mut runtime);
}
