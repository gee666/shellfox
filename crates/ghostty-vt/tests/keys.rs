//! Pins what Ghostty's encoder sends for the keys users press most, in the
//! modes applications request. These are Ghostty's bytes, not a mapping table.
use shellfox_ghostty_vt::key::{KeyAction, KeyEvent, KeyModifiers as M, PhysicalKey as P};
use shellfox_ghostty_vt::Terminal;

fn key(physical: P, mods: M, text: &str, unshifted: Option<char>) -> KeyEvent {
    KeyEvent {
        text: text.into(),
        unshifted_codepoint: unshifted,
        ..KeyEvent::press(physical, mods)
    }
}
fn named(physical: P, mods: M) -> KeyEvent {
    key(physical, mods, "", None)
}
fn letter(c: char, mods: M) -> KeyEvent {
    let physical = P::try_from(20 + u32::from(c.to_ascii_lowercase()) - u32::from('a')).unwrap();
    key(physical, mods, &c.to_string(), Some(c.to_ascii_lowercase()))
}
fn enc(t: &mut Terminal, e: &KeyEvent) -> String {
    String::from_utf8(t.encode_key_event(e).unwrap()).unwrap()
}
const NONE: M = M::NONE;
const SHIFT: M = M::SHIFT;
const CTRL: M = M::CONTROL;
const ALT: M = M::ALT;

#[test]
fn typing_and_non_ascii_text() {
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(enc(&mut t, &letter('a', NONE)), "a");
    assert_eq!(enc(&mut t, &letter('A', SHIFT)), "A");
    assert_eq!(enc(&mut t, &KeyEvent::text("é日本🦊")), "é日本🦊");
    assert_eq!(enc(&mut t, &key(P::Space, NONE, " ", Some(' '))), " ");
    let release = letter('a', NONE).release();
    assert_eq!(release.action, KeyAction::Release);
    assert_eq!(enc(&mut t, &release), "");
}

#[test]
fn control_chords_signals_and_alt() {
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(enc(&mut t, &letter('c', CTRL)), "\x03");
    assert_eq!(enc(&mut t, &letter('d', CTRL)), "\x04");
    assert_eq!(enc(&mut t, &letter('z', CTRL)), "\x1a");
    assert_eq!(enc(&mut t, &letter('a', CTRL)), "\x01");
    // Ctrl with no text still finds the C0 byte from the physical key.
    assert_eq!(enc(&mut t, &named(P::KeyC, CTRL)), "\x03");
    // A non-Latin layout: text is Cyrillic, the physical key is C.
    assert_eq!(enc(&mut t, &key(P::KeyC, CTRL, "с", Some('с'))), "\x03");
    assert_eq!(enc(&mut t, &key(P::Space, CTRL, " ", Some(' '))), "\x00");
    assert_eq!(enc(&mut t, &letter('x', ALT)), "\x1bx");
    assert_eq!(enc(&mut t, &letter('c', CTRL | ALT)), "\x1b\x03");
    // Ghostty follows fixterms: Ctrl+Shift+letter is distinguishable.
    assert_eq!(enc(&mut t, &letter('A', CTRL | SHIFT)), "\x1b[97;6u");
}

#[test]
fn editing_keys() {
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(enc(&mut t, &named(P::Enter, NONE)), "\r");
    assert_eq!(enc(&mut t, &named(P::Tab, NONE)), "\t");
    assert_eq!(enc(&mut t, &named(P::Tab, SHIFT)), "\x1b[Z");
    assert_eq!(enc(&mut t, &named(P::Backspace, NONE)), "\x7f");
    assert_eq!(enc(&mut t, &named(P::Backspace, CTRL)), "\x08");
    assert_eq!(enc(&mut t, &named(P::Backspace, ALT)), "\x1b\x7f");
    assert_eq!(enc(&mut t, &named(P::Escape, NONE)), "\x1b");
    assert_eq!(enc(&mut t, &named(P::Enter, ALT)), "\x1b\r");
}

#[test]
fn navigation_with_modifiers_and_cursor_modes() {
    let mut t = Terminal::new(80, 24).unwrap();
    let cases = [
        (P::ArrowUp, "\x1b[A", "\x1bOA", 'A'),
        (P::ArrowDown, "\x1b[B", "\x1bOB", 'B'),
        (P::ArrowRight, "\x1b[C", "\x1bOC", 'C'),
        (P::ArrowLeft, "\x1b[D", "\x1bOD", 'D'),
        (P::Home, "\x1b[H", "\x1bOH", 'H'),
        (P::End, "\x1b[F", "\x1bOF", 'F'),
    ];
    for (physical, normal, application, last) in cases {
        assert_eq!(enc(&mut t, &named(physical, NONE)), normal);
        assert_eq!(enc(&mut t, &named(physical, SHIFT)), format!("\x1b[1;2{last}"));
        assert_eq!(enc(&mut t, &named(physical, ALT)), format!("\x1b[1;3{last}"));
        assert_eq!(enc(&mut t, &named(physical, CTRL)), format!("\x1b[1;5{last}"));
        assert_eq!(enc(&mut t, &named(physical, CTRL | SHIFT)), format!("\x1b[1;6{last}"));
        t.feed(b"\x1b[?1h").unwrap();
        assert_eq!(enc(&mut t, &named(physical, NONE)), application);
        assert_eq!(enc(&mut t, &named(physical, CTRL)), format!("\x1b[1;5{last}"));
        t.feed(b"\x1b[?1l").unwrap();
    }
    for (physical, code) in [(P::Insert, 2), (P::Delete, 3), (P::PageUp, 5), (P::PageDown, 6)] {
        assert_eq!(enc(&mut t, &named(physical, NONE)), format!("\x1b[{code}~"));
        assert_eq!(enc(&mut t, &named(physical, SHIFT)), format!("\x1b[{code};2~"));
        assert_eq!(enc(&mut t, &named(physical, CTRL)), format!("\x1b[{code};5~"));
        assert_eq!(enc(&mut t, &named(physical, CTRL | ALT)), format!("\x1b[{code};7~"));
    }
}

#[test]
fn function_keys_with_modifiers() {
    let mut t = Terminal::new(80, 24).unwrap();
    let f = |n: u32| P::try_from(120 + n).unwrap();
    for (n, last) in [(1, 'P'), (2, 'Q'), (3, 'R'), (4, 'S')] {
        assert_eq!(enc(&mut t, &named(f(n), NONE)), format!("\x1bO{last}"));
        // Modified F3 as CSI 1;mR would read as a cursor position report.
        let modified = |m: u8| match n {
            3 => format!("\x1b[13;{m}~"),
            _ => format!("\x1b[1;{m}{last}"),
        };
        assert_eq!(enc(&mut t, &named(f(n), SHIFT)), modified(2));
        assert_eq!(enc(&mut t, &named(f(n), CTRL)), modified(5));
    }
    for (n, code) in [(5, 15), (6, 17), (7, 18), (8, 19), (9, 20), (10, 21), (11, 23), (12, 24)] {
        assert_eq!(enc(&mut t, &named(f(n), NONE)), format!("\x1b[{code}~"));
        assert_eq!(enc(&mut t, &named(f(n), SHIFT)), format!("\x1b[{code};2~"));
        assert_eq!(enc(&mut t, &named(f(n), ALT)), format!("\x1b[{code};3~"));
        assert_eq!(enc(&mut t, &named(f(n), CTRL)), format!("\x1b[{code};5~"));
    }
}

#[test]
fn keypad_application_mode() {
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(enc(&mut t, &key(P::Numpad5, NONE, "5", Some('5'))), "5");
    // DECKPAM only applies when the app also turns off mode 1035.
    t.feed(b"\x1b=\x1b[?1035l").unwrap();
    assert_eq!(enc(&mut t, &key(P::Numpad5, NONE, "5", Some('5'))), "\x1bOu");
    assert_eq!(enc(&mut t, &named(P::NumpadEnter, NONE)), "\x1bOM");
}

#[test]
fn kitty_keyboard_protocol_follows_the_application() {
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(enc(&mut t, &named(P::Escape, NONE)), "\x1b");
    // Disambiguate (flag 1), as fish, neovim and helix request.
    t.feed(b"\x1b[>1u").unwrap();
    assert_eq!(enc(&mut t, &named(P::Escape, NONE)), "\x1b[27u");
    assert_eq!(enc(&mut t, &letter('c', CTRL)), "\x1b[99;5u");
    assert_eq!(enc(&mut t, &letter('a', NONE)), "a");
    assert_eq!(enc(&mut t, &named(P::Enter, NONE)), "\r");
    assert_eq!(enc(&mut t, &KeyEvent::text("日本")), "日本");
    // Report event types (flag 2): releases are sent.
    t.feed(b"\x1b[>3u").unwrap();
    assert_eq!(enc(&mut t, &named(P::Escape, NONE).release()), "\x1b[27;1:3u");
    // Popping the flags restores legacy encoding.
    t.feed(b"\x1b[<u\x1b[<u").unwrap();
    assert_eq!(enc(&mut t, &named(P::Escape, NONE)), "\x1b");
    assert_eq!(enc(&mut t, &letter('c', CTRL)), "\x03");
}

#[test]
fn composing_keys_send_nothing() {
    let mut t = Terminal::new(80, 24).unwrap();
    let dead = KeyEvent {
        composing: true,
        ..key(P::Quote, NONE, "´", Some('\''))
    };
    assert_eq!(enc(&mut t, &dead), "");
    assert_eq!(enc(&mut t, &KeyEvent::text("é")), "é");
}

#[test]
fn bracketed_paste_and_focus_follow_modes() {
    let mut t = Terminal::new(80, 24).unwrap();
    assert_eq!(t.encode_paste("echo 1\necho 2").unwrap(), b"echo 1\recho 2");
    assert!(t.encode_focus(true).unwrap().is_empty());
    t.feed(b"\x1b[?2004h\x1b[?1004h").unwrap();
    assert_eq!(
        t.encode_paste("echo 1\r\necho 2").unwrap(),
        b"\x1b[200~echo 1\necho 2\x1b[201~"
    );
    assert_eq!(t.encode_focus(true).unwrap(), b"\x1b[I");
    assert_eq!(t.encode_focus(false).unwrap(), b"\x1b[O");
}

#[test]
fn serde_round_trip_and_invalid_text() {
    let event = letter('A', SHIFT);
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(serde_json::from_str::<KeyEvent>(&json).unwrap(), event);
    assert!(serde_json::from_str::<KeyEvent>(&json.replace("\"physical\":20", "\"physical\":999")).is_err());
    let mut t = Terminal::new(80, 24).unwrap();
    assert!(KeyEvent::text("\x1b[A").validate().is_err());
    assert!(t.encode_key_event(&KeyEvent::text("\x1b[A")).is_err());
}
