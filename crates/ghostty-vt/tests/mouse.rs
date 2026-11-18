use shellfox_ghostty_vt::{
    Error, Modifiers, MouseAction, MouseButton, MouseEvent, MouseGeometry, Terminal,
};

const GEOMETRY: MouseGeometry = MouseGeometry {
    cell_width: 10,
    cell_height: 20,
};
fn event(action: MouseAction, button: Option<MouseButton>) -> MouseEvent {
    MouseEvent {
        action,
        button,
        modifiers: Modifiers::default(),
        x: 25.0,
        y: 65.0,
        any_button_pressed: action != MouseAction::Release && button.is_some(),
    }
}

#[test]
fn tracking_modes_use_real_encoder() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(80, 24)?;
    let press = event(MouseAction::Press, Some(MouseButton::Left));
    let release = event(MouseAction::Release, Some(MouseButton::Left));
    let motion = event(MouseAction::Motion, Some(MouseButton::Left));
    assert!(!terminal.mouse_tracking()?);
    assert!(terminal.encode_mouse(&press, GEOMETRY)?.is_empty());
    terminal.feed(b"\x1b[?9h")?;
    assert!(terminal.mouse_tracking()?);
    assert_eq!(terminal.encode_mouse(&press, GEOMETRY)?, b"\x1b[M #$");
    assert!(terminal.encode_mouse(&release, GEOMETRY)?.is_empty());
    assert!(terminal.encode_mouse(&motion, GEOMETRY)?.is_empty());
    assert!(terminal
        .encode_mouse(
            &event(MouseAction::Press, Some(MouseButton::WheelUp)),
            GEOMETRY
        )?
        .is_empty());
    terminal.feed(b"\x1b[?9l\x1b[?1000h\x1b[?1006h")?;
    assert_eq!(terminal.encode_mouse(&press, GEOMETRY)?, b"\x1b[<0;3;4M");
    assert_eq!(terminal.encode_mouse(&release, GEOMETRY)?, b"\x1b[<0;3;4m");
    assert!(terminal.encode_mouse(&motion, GEOMETRY)?.is_empty());
    terminal.feed(b"\x1b[?1000l\x1b[?1002h")?;
    assert_eq!(terminal.encode_mouse(&motion, GEOMETRY)?, b"\x1b[<32;3;4M");
    // Repeated cell motion is not silently deduplicated by this stateless API.
    assert_eq!(terminal.encode_mouse(&motion, GEOMETRY)?, b"\x1b[<32;3;4M");
    let hover = event(MouseAction::Motion, None);
    assert!(terminal.encode_mouse(&hover, GEOMETRY)?.is_empty());
    terminal.feed(b"\x1b[?1002l\x1b[?1003h")?;
    assert_eq!(terminal.encode_mouse(&hover, GEOMETRY)?, b"\x1b[<35;3;4M");
    terminal.feed(b"\x1b[?1003l")?;
    assert!(!terminal.mouse_tracking()?);
    assert!(terminal.encode_mouse(&press, GEOMETRY)?.is_empty());
    Ok(())
}

#[test]
fn buttons_wheel_modifiers_and_release_identity() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(80, 24)?;
    terminal.feed(b"\x1b[?1000h\x1b[?1006h")?;
    for (button, code) in [
        (MouseButton::Left, 0),
        (MouseButton::Middle, 1),
        (MouseButton::Right, 2),
        (MouseButton::WheelUp, 64),
        (MouseButton::WheelDown, 65),
        (MouseButton::WheelLeft, 66),
        (MouseButton::WheelRight, 67),
        (MouseButton::Back, 128),
        (MouseButton::Forward, 129),
    ] {
        assert_eq!(
            terminal.encode_mouse(&event(MouseAction::Press, Some(button)), GEOMETRY)?,
            format!("\x1b[<{code};3;4M").as_bytes()
        );
    }
    // Public button identities ten and eleven currently have no wire encoding.
    for button in [MouseButton::Ten, MouseButton::Eleven] {
        assert!(terminal
            .encode_mouse(&event(MouseAction::Press, Some(button)), GEOMETRY)?
            .is_empty());
    }
    for (modifiers, code) in [
        (
            Modifiers {
                shift: true,
                ..Default::default()
            },
            4,
        ),
        (
            Modifiers {
                alt: true,
                ..Default::default()
            },
            8,
        ),
        (
            Modifiers {
                control: true,
                ..Default::default()
            },
            16,
        ),
        (
            Modifiers {
                super_key: true,
                ..Default::default()
            },
            0,
        ),
        (
            Modifiers {
                shift: true,
                alt: true,
                control: true,
                super_key: true,
            },
            28,
        ),
    ] {
        let mut e = event(MouseAction::Press, Some(MouseButton::Left));
        e.modifiers = modifiers;
        assert_eq!(
            terminal.encode_mouse(&e, GEOMETRY)?,
            format!("\x1b[<{code};3;4M").as_bytes()
        );
    }
    let right_release = event(MouseAction::Release, Some(MouseButton::Right));
    assert_eq!(
        terminal.encode_mouse(&right_release, GEOMETRY)?,
        b"\x1b[<2;3;4m"
    );
    terminal.feed(b"\x1b[?1006l")?;
    assert_eq!(
        terminal.encode_mouse(&right_release, GEOMETRY)?,
        b"\x1b[M##$"
    );
    Ok(())
}

#[test]
fn coordinates_formats_and_outside_drag() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(300, 30)?;
    terminal.feed(b"\x1b[?1000h")?;
    let mut e = event(MouseAction::Press, Some(MouseButton::Left));
    e.x = 2990.0;
    assert!(terminal.encode_mouse(&e, GEOMETRY)?.is_empty()); // legacy 223-cell limit
    terminal.feed(b"\x1b[?1005h")?;
    assert_eq!(
        terminal.encode_mouse(&e, GEOMETRY)?,
        "\x1b[M \u{14c}$".as_bytes()
    );
    terminal.feed(b"\x1b[?1005l\x1b[?1015h")?;
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY)?, b"\x1b[32;300;4M");
    terminal.feed(b"\x1b[?1015l\x1b[?1006h")?;
    e.x = 0.0;
    e.y = 0.0;
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY)?, b"\x1b[<0;1;1M");
    e.x = -10.0;
    assert!(terminal.encode_mouse(&e, GEOMETRY)?.is_empty());
    e.action = MouseAction::Release;
    e.y = 900.0;
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY)?, b"\x1b[<0;1;30m");
    terminal.feed(b"\x1b[?1000l\x1b[?1002h")?;
    e.action = MouseAction::Motion;
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY)?, b"\x1b[<32;1;30M");
    e.any_button_pressed = false;
    assert!(terminal.encode_mouse(&e, GEOMETRY)?.is_empty());
    terminal.feed(b"\x1b[?1016h")?;
    e = event(MouseAction::Press, Some(MouseButton::Left));
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY)?, b"\x1b[<0;25;65M");
    e.action = MouseAction::Release;
    e.x = -10.0;
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY)?, b"\x1b[<0;-10;65m");
    terminal.resize(80, 24)?;
    terminal.feed(b"\x1b[?1016l\x1b[?1006h")?;
    e.x = 900.0;
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY)?, b"\x1b[<0;80;4m");
    Ok(())
}

#[test]
fn mouse_arguments_are_bounded_before_ffi() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(80, 24)?;
    let mut e = event(MouseAction::Press, Some(MouseButton::Left));
    for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.1e9] {
        e.x = x;
        assert_eq!(terminal.encode_mouse(&e, GEOMETRY).unwrap_err(), Error(-2));
    }
    e.x = 0.0;
    for cell_width in [0, u32::MAX] {
        assert_eq!(
            terminal
                .encode_mouse(
                    &e,
                    MouseGeometry {
                        cell_width,
                        cell_height: 20
                    }
                )
                .unwrap_err(),
            Error(-2)
        );
    }
    e.button = None;
    assert_eq!(terminal.encode_mouse(&e, GEOMETRY).unwrap_err(), Error(-2));
    Ok(())
}
