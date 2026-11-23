//! GPUI keystrokes as each platform delivers them, through Ghostty's encoder.
use super::*;
use gpui::Modifiers;
use shellfox_ghostty_vt::Terminal;

#[derive(Clone, Copy)]
enum Os {
    Linux,
    Mac,
    Windows,
}
const ALL: [Os; 3] = [Os::Linux, Os::Mac, Os::Windows];

fn stroke(spec: &str, key_char: Option<&str>) -> Keystroke {
    let mut stroke = Keystroke::parse(spec).unwrap();
    stroke.key_char = key_char.map(str::to_owned);
    stroke
}
fn event(os: Os, stroke: &Keystroke, action: KeyAction) -> Option<KeyEvent> {
    key_event_for(stroke, action, matches!(os, Os::Mac), matches!(os, Os::Windows))
}
fn bytes(os: Os, terminal: &mut Terminal, stroke: &Keystroke) -> Option<Vec<u8>> {
    let event = event(os, stroke, KeyAction::Press)?;
    Some(terminal.encode_key_event(&event).unwrap())
}
fn sent(os: Os, spec: &str) -> Vec<u8> {
    let mut terminal = Terminal::new(80, 24).unwrap();
    bytes(os, &mut terminal, &stroke(spec, None)).unwrap_or_else(|| panic!("{spec} not sent"))
}

#[test]
fn plain_typing_goes_through_the_input_handler_only() {
    for os in ALL {
        let mac = matches!(os, Os::Mac);
        for (spec, text) in [("a", "a"), ("shift-a", "A"), ("space", " "), ("é", "é"), ("!", "!")] {
            assert!(event(os, &stroke(spec, Some(text)), KeyAction::Press).is_none(), "{spec}");
        }
        // Option composes text on Mac; AltGr (Ctrl+Alt) does on Windows.
        assert_eq!(event(os, &stroke("alt-e", Some("´")), KeyAction::Press).is_none(), mac);
        let altgr = event(os, &stroke("ctrl-alt-q", Some("@")), KeyAction::Press);
        assert_eq!(altgr.is_none(), matches!(os, Os::Windows));
    }
    let commit = KeyEvent::text("é日本🦊");
    let mut terminal = Terminal::new(80, 24).unwrap();
    assert_eq!(terminal.encode_key_event(&commit).unwrap(), "é日本🦊".as_bytes());
}

#[test]
fn control_chords_and_signals() {
    for os in ALL {
        assert_eq!(sent(os, "ctrl-c"), b"\x03");
        assert_eq!(sent(os, "ctrl-d"), b"\x04");
        assert_eq!(sent(os, "ctrl-z"), b"\x1a");
        assert_eq!(sent(os, "ctrl-a"), b"\x01");
        assert_eq!(sent(os, "ctrl-space"), b"\x00");
        assert_eq!(sent(os, "ctrl-\\"), b"\x1c");
        // Ghostty keeps Ctrl+Shift+letter distinct (fixterms).
        assert_eq!(sent(os, "ctrl-shift-a"), b"\x1b[97;6u");
        // GPUI folds Shift into "_": Ctrl+Shift+- is Ctrl+_ (Emacs undo).
        let mut terminal = Terminal::new(80, 24).unwrap();
        let underscore = Keystroke { modifiers: Modifiers::control(), key: "_".into(), key_char: None };
        assert_eq!(bytes(os, &mut terminal, &underscore).unwrap(), b"\x1f");
    }
}

#[test]
fn mac_option_types_its_text_and_is_alt_only_when_it_composes_none() {
    // Option+x on a US layout composes ≈, which the input handler types.
    assert!(event(Os::Mac, &stroke("alt-x", Some("≈")), KeyAction::Press).is_none());
    let mut terminal = Terminal::new(80, 24).unwrap();
    assert_eq!(bytes(Os::Mac, &mut terminal, &stroke("alt-x", None)).unwrap(), b"\x1bx");
}

#[test]
fn alt_chords() {
    // Mac Option sends its composed text instead; see the test above.
    for os in [Os::Linux, Os::Windows] {
        assert_eq!(sent(os, "alt-x"), b"\x1bx");
        assert_eq!(sent(os, "alt-shift-x"), b"\x1bX");
        assert_eq!(sent(os, "alt-."), b"\x1b.");
        assert_eq!(sent(os, "alt-backspace"), b"\x1b\x7f");
    }
    for os in ALL {
        assert_eq!(sent(os, "ctrl-alt-c"), b"\x1b\x03");
        assert_eq!(sent(os, "alt-left"), b"\x1b[1;3D");
    }
}

#[test]
fn editing_navigation_and_function_keys_with_modifiers() {
    for os in ALL {
        let cases: [(&str, &[u8]); 26] = [
            ("enter", b"\r"),
            ("tab", b"\t"),
            ("shift-tab", b"\x1b[Z"),
            ("backspace", b"\x7f"),
            ("ctrl-backspace", b"\x08"),
            ("escape", b"\x1b"),
            ("up", b"\x1b[A"),
            ("shift-up", b"\x1b[1;2A"),
            ("ctrl-left", b"\x1b[1;5D"),
            ("ctrl-right", b"\x1b[1;5C"),
            ("shift-left", b"\x1b[1;2D"),
            ("home", b"\x1b[H"),
            ("shift-end", b"\x1b[1;2F"),
            ("pageup", b"\x1b[5~"),
            ("ctrl-pagedown", b"\x1b[6;5~"),
            ("insert", b"\x1b[2~"),
            ("delete", b"\x1b[3~"),
            ("ctrl-delete", b"\x1b[3;5~"),
            ("f1", b"\x1bOP"),
            ("shift-f1", b"\x1b[1;2P"),
            ("f5", b"\x1b[15~"),
            ("ctrl-f5", b"\x1b[15;5~"),
            ("alt-f9", b"\x1b[20;3~"),
            ("f12", b"\x1b[24~"),
            ("ctrl-shift-f12", b"\x1b[24;6~"),
            ("shift-f3", b"\x1b[13;2~"),
        ];
        for (spec, expected) in cases {
            assert_eq!(sent(os, spec), expected, "{spec}");
        }
    }
}

#[test]
fn application_cursor_keypad_and_kitty_modes_come_from_the_terminal() {
    for os in ALL {
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal.feed(b"\x1b[?1h").unwrap();
        assert_eq!(bytes(os, &mut terminal, &stroke("up", None)).unwrap(), b"\x1bOA");
        assert_eq!(bytes(os, &mut terminal, &stroke("ctrl-up", None)).unwrap(), b"\x1b[1;5A");
        terminal.feed(b"\x1b[?1l\x1b[>1u").unwrap();
        assert_eq!(bytes(os, &mut terminal, &stroke("escape", None)).unwrap(), b"\x1b[27u");
        assert_eq!(bytes(os, &mut terminal, &stroke("ctrl-c", None)).unwrap(), b"\x1b[99;5u");
        assert_eq!(bytes(os, &mut terminal, &stroke("enter", None)).unwrap(), b"\r");
        terminal.feed(b"\x1b[<u").unwrap();
        assert_eq!(bytes(os, &mut terminal, &stroke("escape", None)).unwrap(), b"\x1b");
    }
}

#[test]
fn repeat_and_release_keep_key_identity_without_text() {
    let press = event(Os::Linux, &stroke("ctrl-c", None), KeyAction::Repeat).unwrap();
    assert_eq!(press.action, KeyAction::Repeat);
    assert_eq!(press.text, "c");
    let release = event(Os::Linux, &stroke("shift-up", None), KeyAction::Release).unwrap();
    assert_eq!(release.physical, PhysicalKey::ArrowUp);
    assert!(release.modifiers.contains(KeyModifiers::SHIFT));
    assert!(release.text.is_empty());
    assert_eq!(physical_key("f12"), PhysicalKey::F12);
    assert_eq!(physical_key("f26"), PhysicalKey::Unidentified);
    assert_eq!(physical_key("a"), PhysicalKey::KeyA);
}

#[test]
fn terminal_clipboard_chords_match_explicit_and_mac_native_bindings() {
    use ClipboardAction::{Copy, Paste};
    for mac in [false, true] {
        let action = |key| clipboard_action(&Keystroke::parse(key).unwrap(), mac);
        assert_eq!(action("ctrl-shift-c"), Some(Copy));
        assert_eq!(action("ctrl-shift-v"), Some(Paste));
        assert_eq!(action("cmd-c"), mac.then_some(Copy));
        assert_eq!(action("cmd-v"), mac.then_some(Paste));
        for key in ["ctrl-c", "ctrl-v", "ctrl-alt-shift-c", "cmd-ctrl-shift-v"] {
            assert_eq!(action(key), None, "{key}");
        }
    }
}
