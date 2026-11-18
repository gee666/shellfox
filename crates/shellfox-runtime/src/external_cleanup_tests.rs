use super::*;
use std::time::Instant;
fn local(full: bool, disconnected: bool) -> (Arc<Registry>, Option<mpsc::Receiver<()>>) {
    let (wake, receiver) = mpsc::sync_channel(1);
    if full {
        wake.try_send(()).unwrap();
    }
    let receiver = if disconnected {
        drop(receiver);
        None
    } else {
        Some(receiver)
    };
    // This test registry has no worker, so explicit retry is the only join path.
    (
        Arc::new(Registry {
            slots: Mutex::new(Vec::new()),
            wake,
            notification_failed: AtomicBool::new(false),
        }),
        receiver,
    )
}
fn notification_failure(full: bool, disconnected: bool) {
    let (registry, _receiver) = local(full, disconnected);
    let status = Arc::new(Mutex::new(ExternalStatus::default()));
    let record = registry.reserve(Arc::clone(&status)).unwrap();
    let slot = Arc::clone(&record.slot);
    let completed = Arc::new(AtomicBool::new(false));
    let marker = Arc::clone(&completed);
    let (release, held) = mpsc::sync_channel(1);
    let owner = std::thread::spawn(move || {
        // Controlled cleanup-owner test, not an observer callback latch.
        let _ = held.recv_timeout(Duration::from_secs(10));
        marker.store(true, Ordering::Release);
    });
    let control = record.bind(owner);
    drop(control);
    assert!(
        slot.owner.lock().unwrap().is_some(),
        "original join handle must remain reachable"
    );
    assert!(
        !status.lock().unwrap().closed,
        "notification is not join proof"
    );
    assert!(registry.notification_failed.load(Ordering::Acquire));
    let pending = registry.retry();
    assert_eq!(pending.pending, 1);
    assert_eq!(pending.joined, 0);
    release.try_send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut joined = 0;
    while !status.lock().unwrap().closed {
        assert!(Instant::now() < deadline);
        joined += registry.retry().joined;
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(joined, 1);
    assert!(completed.load(Ordering::Acquire));
    assert!(slot.owner.lock().unwrap().is_none());
    assert_eq!(registry.retry().pending, 0);
}
#[test]
fn full_notification_never_owns_or_loses_the_original_parser_handle() {
    notification_failure(true, false);
}
#[test]
fn disconnected_notification_remains_reachable_and_explicitly_retryable() {
    notification_failure(false, true);
}
#[test]
fn admission_capacity_is_reserved_before_binding_and_unbound_failures_release_it() {
    let (registry, _receiver) = local(false, false);
    let mut records = Vec::new();
    for _ in 0..MAX_RECORDS {
        records.push(
            registry
                .reserve(Arc::new(Mutex::new(ExternalStatus::default())))
                .unwrap(),
        );
    }
    let error = match registry.reserve(Arc::new(Mutex::new(ExternalStatus::default()))) {
        Err(error) => error,
        Ok(_) => panic!("capacity must be bounded"),
    };
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(registry.retry().reserved, MAX_RECORDS);
    records.pop(); // no parser thread ever existed for this unbound reservation
    assert!(registry
        .reserve(Arc::new(Mutex::new(ExternalStatus::default())))
        .is_ok());
}
