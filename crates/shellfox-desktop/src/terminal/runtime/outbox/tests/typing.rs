//! Typing faster than the host acknowledges: nothing is dropped or reordered.
use super::*;
use shellfox_ghostty_vt::key::us_key;

/// The press and release a US keyboard sends for `c`.
fn keystroke(c: char) -> [Command; 2] {
    let (physical, base, shifted) = us_key(c).unwrap();
    let mut modifiers = KeyModifiers::NONE;
    modifiers.set(KeyModifiers::SHIFT, shifted);
    let press = KeyEvent {
        text: c.into(),
        unshifted_codepoint: Some(base),
        consumed_modifiers: modifiers,
        ..KeyEvent::press(physical, modifiers)
    };
    let release = press.release();
    [press, release].map(|event| Command::Key {
        lease: LEASE,
        event,
    })
}

const SAMPLE: &str = "echo The quick brown fox jumps over 13 lazy dogs; ls -la | grep '[a-z]*'\\ ";
fn typed(count: usize) -> String {
    SAMPLE.chars().cycle().take(count).collect()
}

/// What the host writes to the PTY for these requests, in legacy key mode.
fn written(requests: &[Request]) -> Vec<u8> {
    let mut terminal = Terminal::new(80, 24).unwrap();
    key_events(requests)
        .into_iter()
        .flat_map(|event| terminal.encode_key_event(event).unwrap())
        .collect()
}

/// The host takes one request for every `pace` keystrokes typed.
fn type_with_slow_host(queue: &mut Outbox, text: &str, pace: usize) -> Vec<Request> {
    let mut transport = Transport::new();
    let mut requests = Vec::new();
    for (index, c) in text.chars().enumerate() {
        for command in keystroke(c) {
            queue.enqueue(command, Pending::Applied).unwrap();
        }
        transport.flush(queue);
        if index % pace == 0 {
            requests.extend(transport.rx.try_recv().ok().inspect(|request| {
                transport.pending.remove(&request.id.0);
            }));
        }
    }
    requests.extend(transport.finish(queue));
    requests
}

#[test]
fn ten_thousand_keystrokes_typed_faster_than_the_host_acks_all_arrive_in_order() {
    let text = typed(10_000);
    let mut queue = Outbox::default();
    let requests = type_with_slow_host(&mut queue, &text, 8);
    assert_eq!(String::from_utf8(written(&requests)).unwrap(), text);
    assert_eq!(key_events(&requests).len(), 2 * text.len());
    // The backlog went out in batches, not one request per key event.
    assert!(
        requests.len() < text.len() / 4,
        "{} requests",
        requests.len()
    );
    assert!(queue.is_drained());
    assert_eq!(queue.bytes, 0);
}

#[test]
fn ctrl_c_behind_a_long_backlog_is_sent_in_order_within_a_few_requests() {
    let mut queue = Outbox::default();
    let mut transport = Transport::new();
    transport.fill();
    for c in typed(5_000).chars() {
        for command in keystroke(c) {
            queue.enqueue(command, Pending::Applied).unwrap();
        }
    }
    let ctrl_c =
        crate::input::key_event(&gpui::Keystroke::parse("ctrl-c").unwrap(), KeyAction::Press)
            .unwrap();
    queue
        .enqueue(
            Command::Key {
                lease: LEASE,
                event: ctrl_c.clone(),
            },
            Pending::Applied,
        )
        .unwrap();
    transport.drain();
    let requests = transport.finish(&mut queue);
    assert!(requests.len() <= 64, "{} requests", requests.len());
    let events = key_events(&requests);
    assert_eq!(events.len(), 10_001);
    assert_eq!(*events[10_000], ctrl_c);
    let bytes = written(&requests);
    assert_eq!(bytes.last(), Some(&0x03));
}

#[test]
fn typing_pauses_visibly_at_the_memory_bound_and_resumes_after_draining() {
    let mut queue = Outbox::default();
    let mut transport = Transport::new();
    let held =
        crate::input::key_event(&gpui::Keystroke::parse("up").unwrap(), KeyAction::Press).unwrap();
    queue
        .enqueue(
            Command::Key {
                lease: LEASE,
                event: held.clone(),
            },
            Pending::Applied,
        )
        .unwrap();
    let mut accepted = 0;
    'typing: for c in SAMPLE.chars().cycle() {
        for command in keystroke(c) {
            match queue.enqueue(command, Pending::Applied) {
                Ok(()) => {}
                Err(error) => {
                    assert_eq!(error, PAUSED);
                    break 'typing;
                }
            }
        }
        accepted += 1;
    }
    assert!(accepted > 20_000, "paused after {accepted} keystrokes");
    // A held key's release still gets in, so no key stays down.
    queue
        .enqueue(
            Command::Key {
                lease: LEASE,
                event: held.release(),
            },
            Pending::Applied,
        )
        .unwrap();
    assert!(!queue.has_held());
    let [press, _] = keystroke('a');
    assert_eq!(queue.enqueue(press, Pending::Applied), Err(PAUSED));
    // Input resumes once half the backlog has gone to the host.
    while queue.bytes > batch::MAX_PENDING_BYTES / 2 {
        transport.flush(&mut queue);
        transport.drain();
    }
    for command in keystroke('a') {
        queue.enqueue(command, Pending::Applied).unwrap();
    }
    let requests = transport.finish(&mut queue);
    assert_eq!(
        key_events(&requests).last().unwrap().action,
        KeyAction::Release
    );
}
