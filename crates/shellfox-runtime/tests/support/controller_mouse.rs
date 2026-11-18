use super::*;
use shellfox_ghostty_vt::{Modifiers, MouseAction, MouseButton, MouseEvent, MouseGeometry};
#[test]
fn actual_owned_mouse_reports_use_owner_modes_stamped_geometry_and_raw_x10_high_bytes() {
    for (mode, expected) in [
        ("--child-mouse-x10", b"\x1b[M \x85+".as_slice()),
        ("--child-mouse-sgr", b"\x1b[<0;101;11M".as_slice()),
    ] {
        let mut m = Manager::start(mode, "RUNTIME_MOUSE_READY");
        let lease = m.lease();
        assert!(
            matches!(
                m.request(Command::MouseTracking { lease }).unwrap(),
                Response::MouseTracking(true)
            ),
            "controlled PTY must actually deliver tracking mode"
        );
        let initial = m.recover();
        m.ack(&initial).unwrap();
        m.request(Command::Resize {
            lease,
            cols: 120,
            rows: 32,
        })
        .unwrap();
        let snapshot = m.recover();
        m.ack(&snapshot).unwrap();
        let geometry = MouseGeometry {
            cell_width: 1,
            cell_height: 1,
        };
        let event = MouseEvent {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
            x: 100.0,
            y: 10.0,
            any_button_pressed: true,
        };
        let mut stale = stamp(&snapshot);
        stale.cols -= 1;
        assert_eq!(
            m.request(Command::MouseStamped {
                lease,
                event,
                geometry,
                stamp: stale
            })
            .unwrap_err()
            .code,
            ErrorCode::StaleLayout
        );
        assert!(m.probe.input.lock().unwrap().is_empty());
        let id = RequestId(m.host.next);
        let receipt = match m
            .request(Command::MouseStamped {
                lease,
                event,
                geometry,
                stamp: stamp(&snapshot),
            })
            .unwrap()
        {
            Response::Input(receipt) => receipt,
            other => panic!("{other:?}"),
        };
        assert_eq!(receipt.accepted_bytes as usize, expected.len());
        let input = m.probe.input.lock().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0].0, id);
        assert_eq!(input[0].2, expected);
        drop(input);
        let hover = MouseEvent {
            action: MouseAction::Motion,
            button: None,
            any_button_pressed: false,
            ..event
        };
        assert!(matches!(
            m.request(Command::MouseStamped {
                lease,
                event: hover,
                geometry,
                stamp: stamp(&snapshot)
            })
            .unwrap(),
            Response::Input(InputReceipt {
                accepted_bytes: 0,
                ..
            })
        ));
        assert_eq!(
            m.probe.input.lock().unwrap().len(),
            1,
            "ignored motion must not invoke the prewrite hook"
        );
        assert_eq!(
            m.request(Command::MouseStamped {
                lease,
                event: MouseEvent {
                    x: f32::NAN,
                    ..event
                },
                geometry,
                stamp: stamp(&snapshot)
            })
            .unwrap_err()
            .code,
            ErrorCode::Validation
        );
        assert_eq!(m.host.runtime.phase(), Phase::Active);
    }
}
