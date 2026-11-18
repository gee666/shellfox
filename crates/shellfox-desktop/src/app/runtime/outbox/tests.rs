mod limits;
use super::*;
use shellfox_ghostty_vt::{KeyAction, Modifiers, MouseButton, MouseEvent, MouseGeometry, Terminal};
use shellfox_runtime::{COMMAND_CAPACITY, Lease};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

const LEASE: Lease = Lease {
    controller: 7,
    epoch: 1,
};
fn mouse(action: MouseAction, button: MouseButton, x: f32) -> Command {
    Command::Mouse {
        lease: LEASE,
        event: MouseEvent {
            action,
            button: Some(button),
            modifiers: Modifiers::default(),
            x,
            y: 10.,
            any_button_pressed: false,
        },
        geometry: MouseGeometry {
            cell_width: 10,
            cell_height: 20,
        },
    }
}
fn key(action: KeyAction) -> Command {
    Command::Key {
        lease: LEASE,
        event: KeyEvent {
            key: Key::ArrowUp,
            action,
            ..Default::default()
        },
    }
}
struct Transport {
    tx: SyncSender<Request>,
    rx: Receiver<Request>,
    pending: HashMap<u64, Pending>,
    next: u64,
}
impl Transport {
    fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        Self {
            tx,
            rx,
            pending: HashMap::new(),
            next: 0,
        }
    }
    fn fill(&self) {
        for id in 0..COMMAND_CAPACITY {
            self.tx
                .try_send(Request {
                    id: RequestId(10000 + id as u64),
                    command: Command::Screen { lease: LEASE },
                })
                .unwrap();
        }
    }
    fn flush(&mut self, queue: &mut Outbox) {
        assert!(
            queue
                .flush(&mut self.next, &mut self.pending, |request| {
                    self.tx.try_send(request).map_err(|error| match error {
                        TrySendError::Full(request) => SubmitError::Full(request),
                        TrySendError::Disconnected(request) => SubmitError::Disconnected(request),
                    })
                })
                .is_none()
        );
    }
    fn drain(&mut self) -> Vec<Request> {
        let mut requests = Vec::new();
        while let Ok(request) = self.rx.try_recv() {
            self.pending.remove(&request.id.0);
            requests.push(request);
        }
        requests
    }
    fn finish(&mut self, queue: &mut Outbox) -> Vec<Request> {
        let mut result = self.drain();
        for _ in 0..1024 {
            if queue.queue.is_empty() {
                return result;
            }
            self.flush(queue);
            result.extend(self.drain());
        }
        panic!("bounded queue failed to drain");
    }
}
fn saturate_normal(queue: &mut Outbox) {
    for _ in 0..NORMAL_CAPACITY {
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
}

#[test]
fn full_native_and_ui_queues_preserve_mouse_press_release_once_in_order() {
    let mut queue = Outbox::default();
    let mut transport = Transport::new();
    transport.fill();
    saturate_normal(&mut queue);
    queue
        .enqueue(
            mouse(MouseAction::Press, MouseButton::Left, 10.),
            Pending::Applied,
        )
        .unwrap();
    assert_eq!(queue.buttons(), 1);
    transport.flush(&mut queue);
    assert_eq!(transport.next, 0, "real native transport returned Full");
    queue
        .enqueue(
            mouse(MouseAction::Release, MouseButton::Left, 20.),
            Pending::Applied,
        )
        .unwrap();
    assert_eq!(
        queue.buttons(),
        0,
        "release is durably retained before clearing held state"
    );
    queue
        .enqueue(
            mouse(MouseAction::Release, MouseButton::Left, 20.),
            Pending::Applied,
        )
        .unwrap();
    let requests = transport.finish(&mut queue);
    let mut terminal = Terminal::new(80, 24).unwrap();
    terminal.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
    let mut actions = Vec::new();
    for request in requests {
        if let Command::Mouse {
            event, geometry, ..
        } = request.command
        {
            let bytes = terminal.encode_mouse(&event, geometry).unwrap();
            assert!(
                !bytes.is_empty(),
                "the actual Ghostty public encoder accepted each lifecycle event"
            );
            if event.action == MouseAction::Release {
                assert_eq!(bytes.last(), Some(&b'm'));
            }
            actions.push((event.action, event.any_button_pressed));
        }
    }
    assert_eq!(
        actions,
        [(MouseAction::Press, true), (MouseAction::Release, false)]
    );
}

#[test]
fn wheel_burst_and_motion_are_coalesced_without_crossing_release_or_keys() {
    let mut queue = Outbox::default();
    queue
        .enqueue(
            mouse(MouseAction::Press, MouseButton::Left, 1.),
            Pending::Applied,
        )
        .unwrap();
    for x in 0..10000 {
        queue
            .enqueue(
                mouse(MouseAction::Motion, MouseButton::Left, x as f32),
                Pending::Applied,
            )
            .unwrap();
    }
    for button in [MouseButton::WheelUp, MouseButton::WheelRight] {
        for _ in 0..64 {
            queue
                .enqueue(mouse(MouseAction::Press, button, 10.), Pending::Applied)
                .unwrap();
        }
    }
    assert_eq!(queue.queue.len(), 4);
    queue
        .enqueue(key(KeyAction::Press), Pending::Applied)
        .unwrap();
    queue
        .enqueue(
            mouse(MouseAction::Release, MouseButton::Left, 9999.),
            Pending::Applied,
        )
        .unwrap();
    queue
        .enqueue(key(KeyAction::Release), Pending::Applied)
        .unwrap();
    let requests = Transport::new().finish(&mut queue);
    assert_eq!(requests.len(), 133);
    assert!(
        matches!(&requests[1].command, Command::Mouse {event, ..} if event.x == 9999. && event.action == MouseAction::Motion)
    );
    assert!(
        matches!(&requests[130].command, Command::Key {event, ..} if event.action == KeyAction::Press)
    );
    assert!(
        matches!(&requests[131].command, Command::Mouse {event, ..} if event.action == MouseAction::Release)
    );
    assert!(
        matches!(&requests[132].command, Command::Key {event, ..} if event.action == KeyAction::Release)
    );
}

#[test]
fn rejected_press_does_not_hold_and_repeated_cancel_keeps_fixed_bounds() {
    let mut queue = Outbox::default();
    saturate_normal(&mut queue);
    for _ in 0..(CAPACITY - NORMAL_CAPACITY) / 2 {
        queue
            .enqueue(
                mouse(MouseAction::Press, MouseButton::Left, 10.),
                Pending::Applied,
            )
            .unwrap();
        queue
            .enqueue(
                mouse(MouseAction::Release, MouseButton::Left, 10.),
                Pending::Applied,
            )
            .unwrap();
    }
    assert_eq!(queue.queue.len(), CAPACITY);
    assert!(
        queue
            .enqueue(
                mouse(MouseAction::Press, MouseButton::Right, 10.),
                Pending::Applied
            )
            .is_err()
    );
    assert_eq!(queue.buttons(), 0);
    for _ in 0..1000 {
        queue.cancel(false).unwrap();
    }
    assert_eq!(queue.queue.len(), CAPACITY);
}

#[test]
fn held_state_retires_only_after_cleanup_confirmation() {
    let mut queue = Outbox::default();
    queue
        .enqueue(
            mouse(MouseAction::Press, MouseButton::Left, 10.),
            Pending::Applied,
        )
        .unwrap();
    queue
        .enqueue(key(KeyAction::Press), Pending::Applied)
        .unwrap();
    queue.stop_input(false);
    assert_eq!(queue.buttons(), 1);
    assert!(queue.key_release(Key::ArrowUp).is_some());
    queue.stop_input(true);
    assert_eq!(queue.buttons(), 0);
    assert!(queue.key_release(Key::ArrowUp).is_none());
}

#[test]
fn key_release_reservation_and_maximum_wheel_backlog_are_bounded() {
    let mut queue = Outbox::default();
    queue
        .enqueue(key(KeyAction::Press), Pending::Applied)
        .unwrap();
    for _ in 1..NORMAL_CAPACITY {
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
    queue
        .enqueue(key(KeyAction::Release), Pending::Applied)
        .unwrap();
    assert!(queue.key_release(Key::ArrowUp).is_none());
    let requests = Transport::new().finish(&mut queue);
    let keys: Vec<_> = requests
        .iter()
        .filter_map(|request| match &request.command {
            Command::Key { event, .. } => Some(event.action),
            _ => None,
        })
        .collect();
    assert_eq!(keys, [KeyAction::Press, KeyAction::Release]);
    for _ in 0..MAX_WHEEL_STEPS {
        queue
            .enqueue(
                mouse(MouseAction::Press, MouseButton::WheelUp, 10.),
                Pending::Applied,
            )
            .unwrap();
    }
    assert!(
        queue
            .enqueue(
                mouse(MouseAction::Press, MouseButton::WheelUp, 10.),
                Pending::Applied
            )
            .is_err()
    );
    assert_eq!(queue.queue.len(), 1);
    assert_eq!(queue.wheel_steps, MAX_WHEEL_STEPS);
}
