use super::*;
use shellfox_ghostty_vt::{KeyAction, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseGeometry};
use shellfox_host::CapturedSettlement;
use shellfox_runtime::Lease;
fn context(marker: &str, hint: bool) -> NativeInputContext {
    NativeInputContext {
        captured_settlement: CapturedSettlement::Captured(Some(marker.into())),
        user_initiated: true,
        hint,
    }
}
fn lease() -> Lease {
    Lease {
        controller: 1,
        epoch: 2,
    }
}
#[test]
fn full_retry_keeps_marker_and_hint_from_insertion_not_latest_display() {
    let mut queue = Outbox::default();
    let mut displayed = context("2026-11-14T00:00:00.000Z", false);
    queue
        .enqueue_with_input(
            Command::Paste {
                lease: lease(),
                text: "original input".into(),
            },
            Pending::Applied,
            Some(displayed.clone()),
        )
        .unwrap();
    let captured = displayed.clone();
    displayed = context("2026-11-14T00:01:00.000Z", true);
    let mut next = 0;
    let mut pending = HashMap::new();
    queue.flush_with_input(&mut next, &mut pending, |request, input| {
        assert_eq!(input, Some(&captured));
        Err(SubmitError::Full(request))
    });
    queue.flush_with_input(&mut next, &mut pending, |_, input| {
        assert_eq!(input, Some(&captured));
        assert_ne!(input, Some(&displayed));
        Ok(())
    });
    assert_eq!(pending.len(), 1);
}
fn wheel() -> Command {
    Command::Mouse {
        lease: lease(),
        event: MouseEvent {
            action: MouseAction::Press,
            button: Some(MouseButton::WheelUp),
            modifiers: Modifiers::default(),
            x: 1.,
            y: 2.,
            any_button_pressed: false,
        },
        geometry: MouseGeometry {
            cell_width: 8,
            cell_height: 16,
        },
    }
}
#[test]
fn wheel_coalescing_never_crosses_a_captured_marker_or_hint() {
    let mut queue = Outbox::default();
    let first = context("2026-11-14T00:00:00.000Z", false);
    let second = context("2026-11-14T00:01:00.000Z", true);
    for input in [&first, &first, &second] {
        queue
            .enqueue_with_input(wheel(), Pending::Applied, Some(input.clone()))
            .unwrap();
    }
    let mut contexts = Vec::new();
    queue.flush_with_input(&mut 0, &mut HashMap::new(), |_, input| {
        contexts.push(input.cloned().unwrap());
        Ok(())
    });
    assert_eq!(contexts, [first.clone(), first, second]);
}
#[test]
fn cancellation_releases_have_explicit_non_user_context_and_non_input_has_none() {
    let mut queue = Outbox::default();
    let press = context("2026-11-14T00:00:00.000Z", true);
    queue
        .enqueue_with_input(
            Command::Key {
                lease: lease(),
                event: KeyEvent {
                    key: Key::Character('x'),
                    action: KeyAction::Press,
                    ..Default::default()
                },
            },
            Pending::Applied,
            Some(press.clone()),
        )
        .unwrap();
    let synthetic = NativeInputContext {
        user_initiated: false,
        hint: false,
        ..context("2026-11-14T00:01:00.000Z", false)
    };
    queue
        .cancel_with_context(false, Some(synthetic.clone()))
        .unwrap();
    queue
        .enqueue_with_input(Command::RetryClose, Pending::Close, None)
        .unwrap();
    let mut seen = Vec::new();
    queue.flush_with_input(&mut 0, &mut HashMap::new(), |_, input| {
        seen.push(input.cloned());
        Ok(())
    });
    assert_eq!(seen, [Some(press), Some(synthetic), None]);
}
