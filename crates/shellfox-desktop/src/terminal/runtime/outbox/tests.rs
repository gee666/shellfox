mod limits;
mod typing;
mod wheel;
use super::*;
use shellfox_ghostty_vt::key::{KeyAction, KeyEvent, KeyModifiers, PhysicalKey};
use shellfox_ghostty_vt::{Modifiers, MouseButton, MouseEvent, MouseGeometry, Terminal};
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
            action,
            ..KeyEvent::press(PhysicalKey::ArrowUp, KeyModifiers::NONE)
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
/// Queues pastes until input pauses.
fn fill_memory_budget(queue: &mut Outbox) {
    let text = "x".repeat(60_000);
    loop {
        let paste = Command::Paste {
            lease: LEASE,
            text: text.clone(),
        };
        match queue.enqueue(paste, Pending::Applied) {
            Ok(()) => {}
            Err(error) => return assert_eq!(error, PAUSED),
        }
    }
}
/// Every key event sent, single or batched, in order.
fn key_events(requests: &[Request]) -> Vec<&KeyEvent> {
    requests
        .iter()
        .flat_map(|request| match &request.command {
            Command::Key { event, .. } => std::slice::from_ref(event),
            Command::Keys { events, .. } => events.as_slice(),
            _ => &[],
        })
        .collect()
}
/// A backlog the transport cannot take yet.
const BACKLOG: usize = 64;
fn saturate_normal(queue: &mut Outbox) {
    for _ in 0..BACKLOG {
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
    assert_eq!(requests.len(), 7);
    assert!(
        matches!(&requests[1].command, Command::Mouse {event, ..} if event.x == 9999. && event.action == MouseAction::Motion)
    );
    assert!(
        matches!(&requests[2].command, Command::Wheel {event, steps: 64, ..} if event.button == Some(MouseButton::WheelUp))
    );
    assert!(
        matches!(&requests[3].command, Command::Wheel {event, steps: 64, ..} if event.button == Some(MouseButton::WheelRight))
    );
    assert!(
        matches!(&requests[4].command, Command::Key {event, ..} if event.action == KeyAction::Press)
    );
    assert!(
        matches!(&requests[5].command, Command::Mouse {event, ..} if event.action == MouseAction::Release)
    );
    assert!(
        matches!(&requests[6].command, Command::Key {event, ..} if event.action == KeyAction::Release)
    );
}

#[test]
fn rejected_press_does_not_hold_and_repeated_cancel_keeps_fixed_bounds() {
    let mut queue = Outbox::default();
    fill_memory_budget(&mut queue);
    let len = queue.queue.len();
    assert_eq!(
        queue.enqueue(
            mouse(MouseAction::Press, MouseButton::Right, 10.),
            Pending::Applied
        ),
        Err(PAUSED)
    );
    assert_eq!(queue.buttons(), 0);
    for _ in 0..1000 {
        queue.cancel(false).unwrap();
    }
    assert_eq!(queue.queue.len(), len);
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
    assert!(queue.key_release((PhysicalKey::ArrowUp, None)).is_some());
    queue.stop_input(true);
    assert_eq!(queue.buttons(), 0);
    assert!(queue.key_release((PhysicalKey::ArrowUp, None)).is_none());
}

#[test]
fn key_release_reservation_and_maximum_wheel_backlog_are_bounded() {
    let mut queue = Outbox::default();
    queue
        .enqueue(key(KeyAction::Press), Pending::Applied)
        .unwrap();
    for _ in 1..BACKLOG {
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
    assert!(queue.key_release((PhysicalKey::ArrowUp, None)).is_none());
    let requests = Transport::new().finish(&mut queue);
    let keys: Vec<_> = requests
        .iter()
        .filter_map(|request| match &request.command {
            Command::Key { event, .. } => Some(event.action),
            _ => None,
        })
        .collect();
    assert_eq!(keys, [KeyAction::Press, KeyAction::Release]);
    // Each step at a new position needs its own entry; past the bound they drop.
    for x in 0..MAX_WHEEL_ENTRIES + 10 {
        queue
            .enqueue(
                mouse(MouseAction::Press, MouseButton::WheelUp, x as f32),
                Pending::Applied,
            )
            .unwrap();
    }
    assert_eq!(queue.queue.len(), MAX_WHEEL_ENTRIES);
    assert_eq!(queue.wheel_entries, MAX_WHEEL_ENTRIES);
    Transport::new().finish(&mut queue);
    assert_eq!(queue.wheel_entries, 0);
}
