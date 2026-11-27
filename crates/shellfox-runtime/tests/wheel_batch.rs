#![cfg(windows)]
mod support;
use shellfox_ghostty_vt::{Modifiers, MouseAction, MouseButton, MouseEvent, MouseGeometry};
use shellfox_runtime::*;
use support::Harness;

fn accepted(response: Response) -> u32 {
    match response {
        Response::Input(receipt) => receipt.accepted_bytes,
        response => panic!("input receipt expected, got {response:?}"),
    }
}

#[test]
fn a_wheel_batch_writes_every_step_in_one_request() {
    let mut host = Harness::start("--child-mouse-sgr");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_MOUSE_READY"));
    let event = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::WheelDown),
        modifiers: Modifiers::default(),
        x: 4.,
        y: 4.,
        any_button_pressed: false,
    };
    let geometry = MouseGeometry {
        cell_width: 8,
        cell_height: 16,
    };
    let lease = host.lease;
    let one = accepted(
        host.request(Command::Mouse {
            lease,
            event,
            geometry,
        })
        .unwrap(),
    );
    assert!(one > 0, "the fixture enables mouse reporting");
    let batch = accepted(
        host.request(Command::Wheel {
            lease,
            event,
            geometry,
            steps: MAX_WHEEL_BATCH,
        })
        .unwrap(),
    );
    assert_eq!(batch, one * u32::from(MAX_WHEEL_BATCH));
    let left = MouseEvent {
        button: Some(MouseButton::Left),
        ..event
    };
    for (event, steps) in [(event, 0), (event, MAX_WHEEL_BATCH + 1), (left, 2)] {
        let request = Request {
            id: RequestId(999),
            command: Command::Wheel {
                lease,
                event,
                geometry,
                steps,
            },
        };
        assert!(matches!(
            host.runtime.try_send(request),
            Err(SubmitError::Invalid(_))
        ));
    }
}
