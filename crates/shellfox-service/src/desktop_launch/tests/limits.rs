use super::*;

#[test]
fn pending_and_channel_capacity_are_checked_before_creating_metadata() {
    let mut host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    for _ in 0..MAX_PENDING_LAUNCHES {
        router.enqueue(owner, intent(), &host).unwrap();
    }
    assert_eq!(
        router.enqueue(owner, intent(), &host),
        Err(LaunchError::Full)
    );
    assert_eq!(host.creations, 0);
    let mut registration = router.register(owner, &host).unwrap();
    for _ in 0..DESKTOP_CHANNEL_CAPACITY {
        router.dispatch_one(owner, &mut host).unwrap();
    }
    assert_eq!(host.creations, DESKTOP_CHANNEL_CAPACITY);
    assert_eq!(
        router.dispatch_one(owner, &mut host),
        Err(LaunchError::Full)
    );
    assert_eq!(host.creations, DESKTOP_CHANNEL_CAPACITY);
    registration.receiver.try_recv().unwrap();
    router.dispatch_one(owner, &mut host).unwrap();
    assert_eq!(host.creations, DESKTOP_CHANNEL_CAPACITY + 1);
}

#[test]
fn receipt_bound_rejects_new_ids_without_evicting_old_dedup_receipts() {
    let mut host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let mut registration = router.register(owner, &host).unwrap();
    let original =
        DesktopLaunchIntent::new(LaunchAction::Show {}, LaunchDisposition::ExistingWindow);
    for index in 0..MAX_LAUNCH_RECEIPTS {
        let request = if index == 0 {
            original.clone()
        } else {
            DesktopLaunchIntent::new(LaunchAction::Show {}, LaunchDisposition::ExistingWindow)
        };
        let delivery = delivered(&mut router, &mut host, &mut registration, request);
        router.claim(&delivery, &mut host).unwrap();
        router
            .complete(&delivery, LaunchHandling::Handled, &host)
            .unwrap();
    }
    assert_eq!(router.entries.len(), MAX_LAUNCH_RECEIPTS);
    assert_eq!(
        router.enqueue(owner, intent(), &host),
        Err(LaunchError::Full)
    );
    assert_eq!(
        router.enqueue(owner, original, &host).unwrap().state,
        LaunchState::Handled
    );
    assert_eq!(host.creations, 0);
}

#[test]
fn replacing_dropped_receiver_retires_deliveries_and_does_not_move_them() {
    let mut host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    let mut old = router.register(owner, &host).unwrap();
    let delivery = delivered(&mut router, &mut host, &mut old, intent());
    drop(old);
    let mut replacement = router.register(owner, &host).unwrap();
    assert_ne!(replacement.token, delivery.client);
    assert_eq!(
        router
            .receipt(owner, delivery.intent.request_id, &host)
            .unwrap()
            .state,
        LaunchState::Uncertain
    );
    assert!(replacement.receiver.try_recv().is_err());
    assert_eq!(
        router.claim(&delivery, &mut host),
        Err(LaunchError::StaleClient)
    );
}

#[test]
fn known_ui_only_requests_without_receipts_are_not_replayed() {
    let host = HostFake::default();
    let mut router = DesktopLaunchRouter::new();
    let owner = router.owner();
    for action in [
        LaunchAction::Show {},
        LaunchAction::OpenSession {
            session: SessionId::new(),
        },
    ] {
        let request = DesktopLaunchIntent::new(action, LaunchDisposition::NewWindow);
        assert_eq!(
            router.resume(owner, request, &host),
            Err(LaunchError::DeliveryUncertain)
        );
    }
    assert!(router.entries.is_empty());
}
