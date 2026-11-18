use super::*;

#[test]
fn focus_loss_and_capture_cancel_release_once_under_backpressure() {
    for pointer_only in [false, true] {
        let mut queue = Outbox::default();
        let mut transport = Transport::new();
        transport.fill();
        queue
            .enqueue(key(KeyAction::Press), Pending::Applied)
            .unwrap();
        queue
            .enqueue(
                mouse(MouseAction::Press, MouseButton::Left, 10.),
                Pending::Applied,
            )
            .unwrap();
        for _ in 2..NORMAL_CAPACITY {
            queue
                .enqueue(
                    Command::Scroll {
                        lease: LEASE,
                        delta: 1,
                    },
                    Pending::Applied,
                )
                .unwrap();
        }
        queue.cancel(pointer_only).unwrap();
        queue.cancel(pointer_only).unwrap();
        assert_eq!(queue.buttons(), 0);
        assert_eq!(queue.key_release(Key::ArrowUp).is_some(), pointer_only);
        let requests = transport.finish(&mut queue);
        assert_eq!(requests.iter().filter(|r| matches!(&r.command, Command::Mouse {event, ..} if event.action == MouseAction::Release)).count(), 1);
        assert_eq!(requests.iter().filter(|r| matches!(&r.command, Command::Key {event, ..} if event.action == KeyAction::Release)).count(), usize::from(!pointer_only));
    }
}

#[test]
fn all_accepted_physical_keys_keep_reserved_release_slots_at_the_hard_limit() {
    let mut queue = Outbox::default();
    let keys: Vec<_> = (1..=25)
        .map(Key::F)
        .chain(('a'..='z').map(Key::Character))
        .chain(('0'..='9').map(Key::Character))
        .chain([Key::Home, Key::End, Key::Insert])
        .collect();
    assert_eq!(keys.len(), MAX_HELD);
    for key in keys {
        queue
            .enqueue(
                Command::Key {
                    lease: LEASE,
                    event: KeyEvent {
                        key,
                        modifiers: Modifiers {
                            control: true,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                },
                Pending::Applied,
            )
            .unwrap();
    }
    assert!(
        queue
            .enqueue(key(KeyAction::Press), Pending::Applied)
            .is_err()
    );
    queue.cancel(false).unwrap();
    assert_eq!(queue.queue.len(), CAPACITY);
    assert!(!queue.has_held());
    let mut transport = Transport::new();
    transport.fill();
    for _ in 0..10 {
        transport.flush(&mut queue);
    }
    assert_eq!(transport.next, 0);
    let requests = transport.finish(&mut queue);
    let keys: Vec<_> = requests
        .iter()
        .filter_map(|request| match &request.command {
            Command::Key { event, .. } => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(keys.len(), 2 * MAX_HELD);
    assert!(
        keys[..MAX_HELD]
            .iter()
            .all(|event| event.action == KeyAction::Press)
    );
    assert!(
        keys[MAX_HELD..]
            .iter()
            .all(|event| event.action == KeyAction::Release)
    );
    let ids: std::collections::HashSet<_> = requests.iter().map(|request| request.id.0).collect();
    assert_eq!(
        ids.len(),
        requests.len(),
        "Full retries never duplicate an accepted request"
    );
}

#[test]
fn protocol_control_has_separate_bounded_space_without_spending_release_reservations() {
    let mut queue = Outbox::default();
    saturate_normal(&mut queue);
    for _ in 0..CONTROL_CAPACITY {
        queue
            .enqueue(
                Command::Ack {
                    lease: LEASE,
                    sequence: 0,
                },
                Pending::Applied,
            )
            .unwrap();
    }
    assert!(queue.enqueue(Command::RetryClose, Pending::Close).is_err());
    queue
        .enqueue(
            mouse(MouseAction::Press, MouseButton::Left, 10.),
            Pending::Applied,
        )
        .unwrap();
    queue.cancel(true).unwrap();
    assert_eq!(queue.queue.len(), NORMAL_CAPACITY + CONTROL_CAPACITY + 2);
    assert_eq!(queue.buttons(), 0);
    Transport::new().finish(&mut queue);
    assert_eq!(queue.controls, 0);
    queue.enqueue(Command::RetryClose, Pending::Close).unwrap();
}

#[test]
fn cancelling_multiple_buttons_reports_remaining_pressed_state_in_fifo_order() {
    let mut queue = Outbox::default();
    for button in [MouseButton::Left, MouseButton::Right] {
        queue
            .enqueue(mouse(MouseAction::Press, button, 10.), Pending::Applied)
            .unwrap();
    }
    queue.cancel(true).unwrap();
    let flags: Vec<_> = Transport::new()
        .finish(&mut queue)
        .into_iter()
        .filter_map(|request| {
            if let Command::Mouse { event, .. } = request.command {
                Some((event.action, event.button, event.any_button_pressed))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        flags,
        [
            (MouseAction::Press, Some(MouseButton::Left), true),
            (MouseAction::Press, Some(MouseButton::Right), true),
            (MouseAction::Release, Some(MouseButton::Left), true),
            (MouseAction::Release, Some(MouseButton::Right), false),
        ]
    );
}

#[test]
fn changed_shift_or_control_modifiers_do_not_hide_an_accepted_physical_release() {
    let mut queue = Outbox::default();
    queue
        .enqueue(
            Command::Key {
                lease: LEASE,
                event: KeyEvent {
                    key: Key::Character('C'),
                    modifiers: Modifiers {
                        control: true,
                        shift: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
            Pending::Applied,
        )
        .unwrap();
    let release = queue.key_release(crate::input::physical_key("c")).unwrap();
    assert_eq!(release.action, KeyAction::Release);
    queue
        .enqueue(
            Command::Key {
                lease: LEASE,
                event: release,
            },
            Pending::Applied,
        )
        .unwrap();
    assert!(!queue.has_held());
    assert_eq!(Transport::new().finish(&mut queue).len(), 2);
}

#[test]
fn committed_text_does_not_create_synthetic_release_or_duplicate_ime_commit() {
    let mut queue = Outbox::default();
    queue
        .enqueue(
            Command::Key {
                lease: LEASE,
                event: crate::input::committed_text("é🦊"),
            },
            Pending::Applied,
        )
        .unwrap();
    queue.cancel(false).unwrap();
    assert!(!queue.has_held());
    let requests = Transport::new().finish(&mut queue);
    assert_eq!(requests.len(), 1);
    assert!(matches!(&requests[0].command, Command::Key { event, .. }
        if event.action == KeyAction::Press && event.text.as_deref() == Some("é🦊")));
}
