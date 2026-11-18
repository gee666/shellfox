use super::*;
use shellfox_ghostty_vt::{Key, KeyAction, KeyEvent};
use shellfox_runtime::{Command, Lease};

#[test]
fn dropped_view_keeps_retained_intents_then_reserved_release_then_detach() {
    let lease = Lease {
        controller: 7,
        epoch: 3,
    };
    let mut outbox = Outbox::default();
    outbox
        .enqueue(
            Command::Key {
                lease,
                event: KeyEvent {
                    key: Key::Character('x'),
                    action: KeyAction::Press,
                    ..Default::default()
                },
            },
            Pending::Applied,
        )
        .unwrap();
    outbox
        .enqueue(
            Command::Paste {
                lease,
                text: "ordered after press".into(),
            },
            Pending::Applied,
        )
        .unwrap();
    let mut admitted = None;
    let mut next = 0;
    let _ = outbox.flush(&mut next, &mut HashMap::new(), |request| {
        if admitted.is_none() {
            admitted = Some(request);
            Ok(())
        } else {
            Err(SubmitError::Full(request))
        }
    });
    assert!(
        matches!(admitted.unwrap().command, Command::Key { event, .. } if event.action == KeyAction::Press)
    );
    outbox.cancel(false).unwrap();
    outbox.enqueue(Command::RetryClose, Pending::Close).unwrap();
    let mut retirement = Retirement::new(outbox, next);
    assert!(matches!(
        retirement.next().unwrap().command,
        Command::Paste { .. }
    ));
    assert!(
        matches!(retirement.next().unwrap().command, Command::Key { event, .. } if event.action == KeyAction::Release)
    );
    assert!(matches!(
        retirement.next().unwrap().command,
        Command::RetryClose
    ));
    assert!(retirement.next().is_none());
}
