use super::*;
use std::io::{Error, ErrorKind};
fn receipt(result: Result<()>) -> WriteReceipt {
    let (sender, receiver) = mpsc::sync_channel(1);
    sender.send(result).unwrap();
    WriteReceipt { result: receiver }
}
#[test]
fn receipt_preserves_only_broken_pipe_as_typed_closed_and_keeps_other_io() {
    for kind in [
        ErrorKind::BrokenPipe,
        ErrorKind::PermissionDenied,
        ErrorKind::Interrupted,
        ErrorKind::TimedOut,
        ErrorKind::WriteZero,
        ErrorKind::Other,
        ErrorKind::NotConnected,
    ] {
        let error = StdinFailure::from_io(Error::from(kind)).into_error();
        let error = receipt(Err(error.context("Deferred write result")))
            .wait()
            .unwrap_err();
        if kind == ErrorKind::BrokenPipe {
            let closed = error.downcast_ref::<AutomationStdinClosed>().unwrap();
            assert_eq!(closed.io_error().unwrap().kind(), kind);
        } else {
            assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
            assert_eq!(
                error
                    .downcast_ref::<AutomationStdinIoError>()
                    .unwrap()
                    .io_error()
                    .kind(),
                kind
            );
        }
    }
}
#[test]
fn receipt_try_result_never_turns_pending_or_lost_ack_into_success_or_closed() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let receipt = WriteReceipt { result: receiver };
    assert!(receipt.try_result().unwrap().is_none());
    drop(sender);
    let error = receipt.try_result().unwrap_err();
    assert_eq!(
        error.downcast_ref::<AutomationStdinUnavailable>(),
        Some(&AutomationStdinUnavailable::AcknowledgementLost)
    );
    assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
    let error = receipt.wait().unwrap_err();
    assert_eq!(
        error.downcast_ref::<AutomationStdinUnavailable>(),
        Some(&AutomationStdinUnavailable::AcknowledgementLost)
    );
    assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
}
#[test]
fn successful_receipts_and_known_ended_marker_remain_distinct() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let receipt = WriteReceipt { result: receiver };
    assert!(receipt.try_result().unwrap().is_none());
    sender.send(Ok(())).unwrap();
    assert!(receipt.try_result().unwrap().is_some());
    let closed = StdinFailure::known_closed().into_error();
    assert!(closed
        .downcast_ref::<AutomationStdinClosed>()
        .unwrap()
        .io_error()
        .is_none());
}
#[cfg(windows)]
#[test]
fn native_windows_pipe_and_other_failure_codes_are_not_erased() {
    for code in [109, 232, 995, 5] {
        let original = Error::from_raw_os_error(code);
        let kind = original.kind();
        let error = receipt(Err(StdinFailure::from_io(original).into_error()))
            .wait()
            .unwrap_err();
        let native = if kind == ErrorKind::BrokenPipe {
            error
                .downcast_ref::<AutomationStdinClosed>()
                .unwrap()
                .io_error()
                .unwrap()
        } else {
            assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
            error
                .downcast_ref::<AutomationStdinIoError>()
                .unwrap()
                .io_error()
        };
        assert_eq!(native.kind(), kind);
        assert_eq!(native.raw_os_error(), Some(code));
    }
}
