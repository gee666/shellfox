use super::*;
use shellfox_ghostty_vt::Terminal;

fn key(code: KeyCode, modifiers: KeyModifiers) -> Input {
    key_input(Crossterm::new(code, modifiers))
}
fn sent(terminal: &mut Terminal, code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    match key(code, modifiers) {
        Input::Key(event) => terminal.encode_key_event(&event).unwrap(),
        other => panic!("{code:?} {modifiers:?}: {other:?}"),
    }
}

#[test]
fn detach_is_local_but_ctrl_c_is_remote_input() {
    assert_eq!(key(KeyCode::Char(']'), KeyModifiers::CONTROL), Input::Detach);
    assert_eq!(key(KeyCode::Char('\u{1d}'), KeyModifiers::NONE), Input::Detach);
    let release = Crossterm::new_with_kind(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
        KeyEventKind::Release,
    );
    assert_eq!(key_input(release), Input::Ignore);
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(sent(&mut t, KeyCode::Char('c'), KeyModifiers::CONTROL), b"\x03");
    assert_eq!(sent(&mut t, KeyCode::Char(']'), KeyModifiers::NONE), b"]");
}

#[cfg(unix)]
#[test]
fn unix_group_separator_decoded_by_crossterm_detaches() {
    assert_eq!(key(KeyCode::Char('5'), KeyModifiers::CONTROL), Input::Detach);
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(sent(&mut t, KeyCode::Char('5'), KeyModifiers::NONE), b"5");
}

#[test]
fn typing_chords_and_navigation_use_ghostty_with_live_modes() {
    use KeyModifiers as M;
    let mut t = Terminal::new(80, 24).unwrap();
    let cases: [(KeyCode, M, &[u8]); 22] = [
        (KeyCode::Char('界'), M::NONE, "界".as_bytes()),
        (KeyCode::Char('A'), M::SHIFT, b"A"),
        (KeyCode::Char('x'), M::ALT, b"\x1bx"),
        (KeyCode::Char('d'), M::CONTROL, b"\x04"),
        (KeyCode::Char('z'), M::CONTROL, b"\x1a"),
        (KeyCode::Char(' '), M::CONTROL, b"\x00"),
        (KeyCode::Enter, M::NONE, b"\r"),
        (KeyCode::Tab, M::NONE, b"\t"),
        (KeyCode::BackTab, M::SHIFT, b"\x1b[Z"),
        (KeyCode::Backspace, M::NONE, b"\x7f"),
        (KeyCode::Backspace, M::CONTROL, b"\x08"),
        (KeyCode::Esc, M::NONE, b"\x1b"),
        (KeyCode::Up, M::NONE, b"\x1b[A"),
        (KeyCode::Left, M::CONTROL, b"\x1b[1;5D"),
        (KeyCode::Right, M::SHIFT, b"\x1b[1;2C"),
        (KeyCode::Home, M::NONE, b"\x1b[H"),
        (KeyCode::End, M::SHIFT, b"\x1b[1;2F"),
        (KeyCode::PageUp, M::NONE, b"\x1b[5~"),
        (KeyCode::Delete, M::CONTROL, b"\x1b[3;5~"),
        (KeyCode::F(1), M::NONE, b"\x1bOP"),
        (KeyCode::F(5), M::NONE, b"\x1b[15~"),
        (KeyCode::F(12), M::CONTROL, b"\x1b[24;5~"),
    ];
    for (code, modifiers, expected) in cases {
        assert_eq!(sent(&mut t, code, modifiers), expected, "{code:?} {modifiers:?}");
    }
    t.feed(b"\x1b[?1h").unwrap();
    assert_eq!(sent(&mut t, KeyCode::Up, M::NONE), b"\x1bOA");
    t.feed(b"\x1b[?1l\x1b[>1u").unwrap();
    assert_eq!(sent(&mut t, KeyCode::Esc, M::NONE), b"\x1b[27u");
}

#[test]
fn response_looking_keys_and_pastes_are_not_stripped_or_reframed() {
    let replies = "\x1b[?61;6;7;22c\x1b[12;34R\x1b[4;900;1200t\x1b]10;rgb:ff/ff/ff\x1b\\";
    let mut t = Terminal::new(80, 24).unwrap();
    let typed: Vec<u8> = replies
        .chars()
        .flat_map(|c| match from_event(Event::Key(Crossterm::new(KeyCode::Char(c), KeyModifiers::NONE))) {
            Input::Key(event) => t.encode_key_event(&event).unwrap(),
            // ESC and other controls arrive as their own keys, not text.
            Input::Ignore if c.is_control() => Vec::new(),
            other => panic!("user key was classified as {other:?}"),
        })
        .collect();
    assert_eq!(typed, replies.replace(|c: char| c.is_control(), "").as_bytes());
    // Event::Paste is already framed by crossterm. Its payload is opaque.
    let paste = format!("{replies}\x1d\n雪🙂");
    assert_eq!(from_event(Event::Paste(paste.clone())), Input::Paste(paste));
}

#[test]
fn release_keeps_identity_without_text() {
    let release = Crossterm::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Release);
    let Input::Key(event) = key_input(release) else { panic!() };
    assert_eq!(event.action, KeyAction::Release);
    assert_eq!(event.physical, P::KeyA);
    assert!(event.text.is_empty());
    let mut t = Terminal::new(80, 24).unwrap();
    assert!(t.encode_key_event(&event).unwrap().is_empty());
    assert_eq!(key(KeyCode::Null, KeyModifiers::NONE), Input::Ignore);
}
