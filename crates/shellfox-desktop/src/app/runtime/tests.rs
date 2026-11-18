use super::*;
use shellfox_runtime::{Request, RequestId};
#[test]
fn only_typed_stale_presentation_requests_semantic_refresh() {
    let stale: anyhow::Error = shellfox_runtime::RuntimeError {
        code: ErrorCode::StalePresentation,
        message: "version changed".into(),
    }
    .into();
    assert!(stale_presentation(&stale));
    assert!(!stale_presentation(&anyhow::anyhow!("version changed")));
    let denied: anyhow::Error = shellfox_runtime::RuntimeError {
        code: ErrorCode::ControlRequired,
        message: "blocked".into(),
    }
    .into();
    assert!(!stale_presentation(&denied));
}
#[test]
fn acknowledgements_never_credit_undelivered_output() {
    let mut state = DeliveryState {
        consumed: 20,
        delivered: 4,
        ..Default::default()
    };
    assert_eq!(state.acknowledgement(), Some(4));
    state.acked = Some(4);
    assert_eq!(state.acknowledgement(), None);
    state.delivered = 30;
    assert_eq!(state.acknowledgement(), Some(20));
    state.ack_pending = true;
    assert_eq!(state.acknowledgement(), None);
}
#[test]
fn close_waits_for_final_delivery_and_authoritative_frame() {
    let mut state = DeliveryState {
        consumed: 2,
        delivered: 4,
        exited: Some(4),
        ..Default::default()
    };
    assert!(!state.finished());
    state.consumed = 4;
    assert!(state.finished());
    state.delivered = 3;
    assert!(!state.finished());
    state.exited = None;
    assert!(!state.finished());
}

#[test]
fn ansi_recovery_waits_for_its_authoritative_frame_before_acknowledging() {
    let mut state = DeliveryState {
        consumed: 10,
        delivered: 40,
        recovery_barrier: Some(40),
        ..Default::default()
    };
    assert_eq!(state.acknowledgement(), None);
    state.consumed = 50;
    assert_eq!(state.acknowledgement(), Some(40));
}

#[test]
fn zero_sequence_recovery_still_requires_an_acknowledgement() {
    let state = DeliveryState {
        recovery_barrier: Some(0),
        ..Default::default()
    };
    assert_eq!(state.acknowledgement(), Some(0));
}

#[test]
fn submission_errors_never_include_rejected_input() {
    let error = SubmitError::Disconnected(Request {
        id: RequestId(9),
        command: Command::Paste {
            lease: shellfox_runtime::Lease {
                controller: 1,
                epoch: 1,
            },
            text: "private clipboard text".into(),
        },
    });
    assert_eq!(submission_error(error), "Terminal owner disconnected");
}
