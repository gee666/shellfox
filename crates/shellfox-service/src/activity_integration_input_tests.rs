use super::*;
use crate::activity_integration::{ActivityClock, LiveActivitySink};
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_ghostty_vt::{KeyAction, MouseButton};
use shellfox_runtime::{ErrorCode, Identity, LiveOutputSink, Response, RuntimeError};
use std::{sync::Arc, time::Instant};
fn fixture() -> (Identity, LiveActivitySink, Arc<ActivityClock>) {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let clock = Arc::new(ActivityClock::new(Instant::now(), 0));
    (
        identity,
        LiveActivitySink::new(identity, Arc::clone(&clock)),
        clock,
    )
}
fn positive(bytes: u32, kind: InputKind) -> Result<Response, RuntimeError> {
    Ok(Response::Input(InputReceipt {
        accepted_bytes: bytes,
        kind,
    }))
}
fn captured() -> CapturedSettlement {
    CapturedSettlement::Captured(Some("2026-01-01T00:00:00.000Z".into()))
}

#[test]
fn zero_receipt_never_marks_echo_or_touches_and_original_null_is_not_legacy() {
    let (identity, sink, clock) = fixture();
    sink.register_input(
        identity,
        RequestId(1),
        InputAdmission::native(true, true, CapturedSettlement::Captured(None)),
    )
    .unwrap();
    let kind = InputKind::Key {
        action: KeyAction::Release,
        has_committed_text: false,
    };
    let accepted = sink
        .finish_input(identity, RequestId(1), &positive(0, kind))
        .unwrap()
        .unwrap();
    assert!(!accepted.activity.touch_session);
    assert_eq!(
        accepted.admission.captured_settlement,
        CapturedSettlement::Captured(None)
    );
    sink.output(identity, 1, b"work", clock.instant_at(100).unwrap())
        .unwrap();
    assert_eq!(sink.take_update().unwrap().busy_changed, Some(true));
}
#[test]
fn exact_prewrite_marks_echo_but_only_receipt_accepts_and_never_recaptures_token_hint() {
    let (identity, sink, clock) = fixture();
    sink.register_input(
        identity,
        RequestId(1),
        InputAdmission::raw(b"x", true, false, captured()).unwrap(),
    )
    .unwrap();
    sink.before_input(
        identity,
        RequestId(1),
        InputKind::Raw,
        b"x",
        clock.instant_at(200).unwrap(),
    )
    .unwrap();
    sink.output(identity, 1, b"echo", clock.instant_at(200).unwrap())
        .unwrap();
    let echo = sink.take_update().unwrap();
    assert_eq!(echo.output_at_ms, Some(200));
    assert_eq!(echo.meaningful_at_ms, None);
    let accepted = sink
        .finish_input(identity, RequestId(1), &positive(1, InputKind::Raw))
        .unwrap()
        .unwrap();
    assert_eq!(accepted.identity, identity);
    assert_eq!(accepted.request_id.0, 1);
    assert!(accepted.activity.touch_session);
    assert!(!accepted.admission.hint);
    assert_eq!(accepted.admission.captured_settlement, captured());
    assert!(sink
        .finish_input(identity, RequestId(1), &positive(1, InputKind::Raw))
        .is_err());
}
#[test]
fn bounded_book_cleans_submission_failure_error_zero_and_close_without_legacy_fallback() {
    let (identity, sink, _) = fixture();
    for request in 0..COMMAND_CAPACITY {
        sink.register_input(
            identity,
            RequestId(request as u64),
            InputAdmission::native(true, true, captured()),
        )
        .unwrap();
    }
    assert!(matches!(
        sink.register_input(
            identity,
            RequestId(99),
            InputAdmission::native(true, true, captured())
        ),
        Err(InputContextFailure::Full)
    ));
    assert!(matches!(
        sink.register_input(
            identity,
            RequestId(0),
            InputAdmission::native(true, true, captured())
        ),
        Err(InputContextFailure::Duplicate)
    ));
    assert!(sink.cancel_input(identity, RequestId(0)));
    assert!(!sink.cancel_input(identity, RequestId(0)));
    let error = Err(RuntimeError {
        code: ErrorCode::Validation,
        message: "rejected input".into(),
    });
    assert!(sink
        .finish_input(identity, RequestId(1), &error)
        .unwrap()
        .is_none());
    assert!(!sink.cancel_input(identity, RequestId(1)));
    let zero = positive(0, InputKind::Paste { nonempty: false });
    assert!(sink
        .finish_input(identity, RequestId(2), &zero)
        .unwrap()
        .is_some());
    assert_eq!(sink.cancel_all_inputs(identity), COMMAND_CAPACITY - 3);
    assert!(matches!(
        sink.finish_input(identity, RequestId(3), &positive(1, InputKind::Raw)),
        Err(InputContextFailure::Missing)
    ));
}
#[test]
fn disabled_optional_reporting_preserves_the_single_book_and_atomic_cas_context() {
    let (identity, sink, _) = fixture();
    sink.disable();
    sink.register_input(
        identity,
        RequestId(1),
        InputAdmission::raw(b"x", true, true, captured()).unwrap(),
    )
    .unwrap();
    let accepted = sink
        .finish_input(identity, RequestId(1), &positive(1, InputKind::Raw))
        .unwrap()
        .unwrap();
    assert!(accepted.activity.touch_session);
    assert_eq!(accepted.admission.captured_settlement, captured());
    let hover = InputKind::Mouse {
        action: MouseAction::Motion,
        button: None,
        any_button_pressed: false,
    };
    sink.register_input(
        identity,
        RequestId(2),
        InputAdmission::native(true, true, captured()),
    )
    .unwrap();
    assert!(
        !sink
            .finish_input(identity, RequestId(2), &positive(8, hover))
            .unwrap()
            .unwrap()
            .activity
            .touch_session
    );
    let click = InputKind::Mouse {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        any_button_pressed: true,
    };
    sink.register_input(
        identity,
        RequestId(3),
        InputAdmission::native(true, false, captured()),
    )
    .unwrap();
    assert!(
        sink.finish_input(identity, RequestId(3), &positive(8, click))
            .unwrap()
            .unwrap()
            .activity
            .touch_session
    );
}
#[test]
fn encoded_passivity_overrides_hint_and_prewrite_failure_is_not_positive_acceptance() {
    let (identity, sink, clock) = fixture();
    let bytes = b"\x1b[<35;1;1M";
    let kind = InputKind::Mouse {
        action: MouseAction::Motion,
        button: None,
        any_button_pressed: false,
    };
    sink.register_input(
        identity,
        RequestId(1),
        InputAdmission::native(true, true, captured()),
    )
    .unwrap();
    sink.before_input(
        identity,
        RequestId(1),
        kind,
        bytes,
        clock.instant_at(0).unwrap(),
    )
    .unwrap();
    let accepted = sink
        .finish_input(
            identity,
            RequestId(1),
            &positive(u32::try_from(bytes.len()).unwrap(), kind),
        )
        .unwrap()
        .unwrap();
    assert!(accepted.admission.hint);
    assert!(!accepted.activity.touch_session);
    sink.register_input(
        identity,
        RequestId(2),
        InputAdmission::raw(b"x", true, true, captured()).unwrap(),
    )
    .unwrap();
    sink.before_input(
        identity,
        RequestId(2),
        InputKind::Raw,
        b"x",
        clock.instant_at(1).unwrap(),
    )
    .unwrap();
    let error = Err(RuntimeError {
        code: ErrorCode::Unavailable,
        message: "write failed".into(),
    });
    assert!(sink
        .finish_input(identity, RequestId(2), &error)
        .unwrap()
        .is_none());
}
#[test]
fn receipt_mismatch_stale_generation_and_oversized_admission_fail_closed() {
    let (identity, sink, clock) = fixture();
    let stale = Identity {
        generation: Generation::new(),
        ..identity
    };
    assert!(matches!(
        sink.register_input(
            stale,
            RequestId(1),
            InputAdmission::native(true, true, captured())
        ),
        Err(InputContextFailure::Stale)
    ));
    assert!(InputAdmission::raw(
        &vec![0; MAX_ENCODED_INPUT_BYTES + 1],
        true,
        true,
        captured()
    )
    .is_err());
    sink.register_input(
        identity,
        RequestId(1),
        InputAdmission::raw(b"x", true, true, captured()).unwrap(),
    )
    .unwrap();
    sink.before_input(
        identity,
        RequestId(1),
        InputKind::Raw,
        b"x",
        clock.instant_at(0).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        sink.finish_input(identity, RequestId(1), &positive(2, InputKind::Raw)),
        Err(InputContextFailure::InvalidReceipt)
    ));
    assert!(!sink.cancel_input(identity, RequestId(1)));
    assert_eq!(sink.status(), (false, false));
}
