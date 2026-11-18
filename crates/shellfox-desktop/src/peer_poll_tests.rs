use super::*;
#[tokio::test]
async fn routine_drain_retains_original_poll_until_completion_without_cancellation() {
    let cancel = CancellationToken::new();
    let observed = cancel.clone();
    let (done, receive) = oneshot::channel();
    let mut slot = PollSlot {
        cancel,
        done: Some(receive),
    };
    let draining = slot.drain().unwrap();
    assert!(!observed.is_cancelled());
    done.send(()).unwrap();
    draining.await.unwrap();
    assert!(!observed.is_cancelled());
    drop(slot);
    assert!(observed.is_cancelled());
}
#[tokio::test]
async fn missing_drain_completion_is_unknown_not_a_successful_drain() {
    let cancel = CancellationToken::new();
    let (done, receive) = oneshot::channel();
    let mut slot = PollSlot {
        cancel: cancel.clone(),
        done: Some(receive),
    };
    let draining = slot.drain().unwrap();
    drop(done);
    assert!(draining.await.is_err());
    assert!(!cancel.is_cancelled());
    // Production attach handles this error by retiring affected views and
    // requiring fresh native recovery, never by resuming the old delivery.
    drop(slot);
    assert!(cancel.is_cancelled());
}
