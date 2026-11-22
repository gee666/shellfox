use shellfox_ghostty_vt::{Error, Key, KeyAction, KeyEvent, Modifiers, Rgb, Terminal};

#[test]
fn fragmented_utf8_escape_and_owned_render() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(12, 3)?;
    let stream = "A\x1b[1;3;4;38;2;1;2;3m界e\u{301}\x1b[0m";
    for byte in stream.as_bytes() {
        terminal.feed(&[*byte])?;
    }
    let screen = terminal.render()?;
    assert_eq!(screen.cells[0].text, "A");
    assert_eq!(screen.cells[1].text, "界");
    assert_eq!(screen.cells[1].width, 2);
    assert_eq!(screen.cells[2].width, 0);
    assert_eq!(screen.cells[3].text, "e\u{301}");
    assert!(screen.cells[1].bold && screen.cells[1].italic && screen.cells[1].underline);
    assert_eq!(screen.cells[1].foreground, Rgb { r: 1, g: 2, b: 3 });
    terminal.feed(b"\x1b[2J\x1b[Hnew")?;
    assert_eq!(terminal.render()?.cells[0].text, "n");
    drop(terminal);
    assert_eq!(screen.cells[1].text, "界");
    Ok(())
}

#[test]
fn replies_are_owned_and_drained_once_not_on_render() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(80, 24)?;
    terminal.feed(b"abc\x1b[")?;
    assert!(terminal.drain_replies()?.is_empty());
    terminal.feed(b"6n")?;
    assert_eq!(terminal.drain_replies()?, b"\x1b[1;4R");
    terminal.render()?;
    terminal.render()?;
    assert!(terminal.drain_replies()?.is_empty());
    terminal.feed(b"\x1b[5n")?;
    let reply = terminal.drain_replies()?;
    drop(terminal);
    assert_eq!(reply, b"\x1b[0n");
    Ok(())
}

#[test]
fn alternate_screen_resize_and_cursor() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(10, 3)?;
    terminal.feed(b"primary\x1b[?1049h\x1b[Halternate\x1b[?25l")?;
    let screen = terminal.render()?;
    assert_eq!(screen.cells[0].text, "a");
    assert!(!screen.cursor.visible);
    terminal.resize(14, 4)?;
    assert_eq!(terminal.render()?.cells.len(), 56);
    terminal.feed(b"\x1b[?1049l\x1b[?25h")?;
    let screen = terminal.render()?;
    assert_eq!(screen.cells[0].text, "p");
    assert!(screen.cursor.visible);
    assert!(terminal.resize(0, 4).is_err());
    assert_eq!(terminal.render()?.cols, 14);
    Ok(())
}

#[test]
fn ghostty_key_modes_ctrl_c_release_and_paste() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(80, 24)?;
    let mut key = KeyEvent {
        key: Key::ArrowUp,
        ..Default::default()
    };
    assert_eq!(terminal.encode_key(&key)?, b"\x1b[A");
    terminal.feed(b"\x1b[?1h")?;
    assert_eq!(terminal.encode_key(&key)?, b"\x1bOA");
    key = KeyEvent {
        key: Key::Character('c'),
        text: Some("c".into()),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(terminal.encode_key(&key)?, b"\x03");
    key.action = KeyAction::Release;
    assert!(terminal.encode_key(&key)?.is_empty());
    assert_eq!(terminal.encode_paste("hello")?, b"hello");
    // User paste is never refused; unbracketed newlines become CR, ESC a space.
    assert_eq!(terminal.encode_paste("a\nb\x1b")?, b"a\rb ");
    assert_eq!(terminal.encode_paste("a\r\nb\r")?, b"a\rb\r");
    terminal.feed(b"\x1b[?2004h")?;
    assert_eq!(terminal.encode_paste("a\nb")?, b"\x1b[200~a\nb\x1b[201~");
    assert_eq!(terminal.encode_paste("a\r\nb")?, b"\x1b[200~a\nb\x1b[201~");
    Ok(())
}

#[test]
fn query_storm_fails_closed_and_stays_poisoned() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(80, 24)?;
    let query = b"\x1b[6n".repeat(120_000);
    terminal.feed(&query)?;
    assert_eq!(terminal.feed(&query).unwrap_err(), Error(-6));
    assert_eq!(terminal.feed(b"text").unwrap_err(), Error(-6));
    assert!(terminal.render().is_err());
    assert!(terminal.resize(90, 25).is_err());
    assert_eq!(terminal.drain_replies().unwrap_err(), Error(-6));
    assert_eq!(
        terminal.encode_key(&KeyEvent::default()).unwrap_err(),
        Error(-6)
    );
    assert_eq!(terminal.encode_paste("text").unwrap_err(), Error(-6));
    assert_eq!(terminal.scroll(-1).unwrap_err(), Error(-6));
    assert_eq!(terminal.mouse_tracking().unwrap_err(), Error(-6));
    assert_eq!(terminal.copy_selection().unwrap_err(), Error(-6));
    assert_eq!(terminal.set_selection(None).unwrap_err(), Error(-6));
    assert_eq!(
        terminal
            .hyperlink_at(shellfox_ghostty_vt::ViewportPoint { x: 0, y: 0 })
            .unwrap_err(),
        Error(-6)
    );
    let event = shellfox_ghostty_vt::MouseEvent {
        action: shellfox_ghostty_vt::MouseAction::Press,
        button: Some(shellfox_ghostty_vt::MouseButton::Left),
        modifiers: Modifiers::default(),
        x: 0.0,
        y: 0.0,
        any_button_pressed: true,
    };
    assert_eq!(
        terminal
            .encode_mouse(
                &event,
                shellfox_ghostty_vt::MouseGeometry {
                    cell_width: 10,
                    cell_height: 20
                }
            )
            .unwrap_err(),
        Error(-6)
    );
    let colors = shellfox_ghostty_vt::TerminalColors {
        foreground: Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        background: Rgb { r: 0, g: 0, b: 0 },
        cursor: None,
        palette: None,
    };
    assert_eq!(terminal.set_colors(&colors).unwrap_err(), Error(-6));
    assert_eq!(
        terminal.format_vt(0xFFF, 1024 * 1024).unwrap_err(),
        Error(-6)
    );
    assert_eq!(terminal.format_vt(u32::MAX, 0).unwrap_err(), Error(-6));
    assert_eq!(terminal.continuation().unwrap_err(), Error(-6));
    assert_eq!(terminal.recovery_info().unwrap_err(), Error(-6));
    assert_eq!(terminal.take_metadata().unwrap_err(), Error(-6));
    // Poison wins even over malformed arguments. No operation revives state.
    assert_eq!(terminal.resize(0, 0).unwrap_err(), Error(-6));
    assert_eq!(
        terminal
            .hyperlink_at(shellfox_ghostty_vt::ViewportPoint { x: u16::MAX, y: 0 })
            .unwrap_err(),
        Error(-6)
    );
    assert_eq!(
        terminal
            .encode_key(&KeyEvent {
                key: Key::F(0),
                ..Default::default()
            })
            .unwrap_err(),
        Error(-6)
    );
    assert_eq!(
        terminal
            .encode_paste(&"x".repeat(1024 * 1024 + 1))
            .unwrap_err(),
        Error(-6)
    );
    Ok(())
}

#[test]
fn invalid_inputs_and_malformed_output_do_not_panic() -> Result<(), Box<dyn std::error::Error>> {
    assert!(Terminal::new(0, 24).is_err());
    assert!(Terminal::new(u16::MAX, u16::MAX).is_err());
    let mut terminal = Terminal::new(8, 3)?;
    let invalid = KeyEvent {
        key: Key::F(0),
        ..Default::default()
    };
    assert!(terminal.encode_key(&invalid).is_err());
    let invalid = KeyEvent {
        text: Some("\0".into()),
        ..Default::default()
    };
    assert!(terminal.encode_key(&invalid).is_err());
    assert!(terminal.feed(&vec![b'x'; 1024 * 1024 + 1]).is_err());
    terminal.feed(&[0xff, 0xc0, 0x80, 0x1b, b'[', b'm'])?;
    assert_eq!(terminal.render()?.cells.len(), 24);
    Ok(())
}
