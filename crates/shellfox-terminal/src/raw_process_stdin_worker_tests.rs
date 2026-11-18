use super::*;
fn input(capacity: usize) -> (Input, mpsc::Receiver<WriteRequest>) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    (
        Input {
            sender,
            closed: Arc::new(AtomicBool::new(false)),
            thread: None,
            cancellation: Arc::new(Mutex::new(None)),
            failure: Arc::new(Mutex::new(None)),
        },
        receiver,
    )
}
#[test]
fn full_queue_sender_disconnect_and_lost_receipt_are_three_nonclosure_outcomes() {
    let (input, incoming) = input(1);
    let first = input.write(vec![1], false).unwrap();
    assert!(first.try_result().unwrap().is_none());
    let error = input.write(vec![2], false).err().unwrap();
    assert_eq!(
        error.downcast_ref::<AutomationStdinUnavailable>(),
        Some(&AutomationStdinUnavailable::QueueFull)
    );
    assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
    drop(incoming);
    let error = first.wait().unwrap_err();
    assert_eq!(
        error.downcast_ref::<AutomationStdinUnavailable>(),
        Some(&AutomationStdinUnavailable::AcknowledgementLost)
    );
    assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
    let error = input.write(vec![3], false).err().unwrap();
    assert_eq!(
        error.downcast_ref::<AutomationStdinUnavailable>(),
        Some(&AutomationStdinUnavailable::WriterDisconnected)
    );
    assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
}
#[test]
fn nonpipe_terminal_io_failure_stays_uncertain_for_later_writes_and_close() {
    let (input, _incoming) = input(1);
    *input.failure.lock().unwrap() =
        Some(StdinFailure::from_io(std::io::Error::from_raw_os_error(5)));
    input.closed.store(true, Ordering::Release);
    for close in [false, true] {
        if close {
            input.close();
        }
        let error = input.write(vec![1], false).err().unwrap();
        assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
        let io = error
            .downcast_ref::<AutomationStdinIoError>()
            .unwrap()
            .io_error();
        assert_eq!(io.raw_os_error(), Some(5));
        assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied);
    }
}
#[test]
fn explicit_ended_input_is_known_closed_but_does_not_ack_a_queued_write() {
    let (input, incoming) = input(1);
    let pending = input.write(vec![1], false).unwrap();
    input.close();
    let error = input.write(vec![2], false).err().unwrap();
    assert!(error.downcast_ref::<AutomationStdinClosed>().is_some());
    assert!(pending.try_result().unwrap().is_none());
    drop(incoming);
    let error = pending.wait().unwrap_err();
    assert_eq!(
        error.downcast_ref::<AutomationStdinUnavailable>(),
        Some(&AutomationStdinUnavailable::AcknowledgementLost)
    );
    assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
}
