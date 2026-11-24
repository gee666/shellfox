//! Non-Latin layouts and arbitrary text through the desktop key path.
use super::*;
use gpui::Modifiers;
use shellfox_ghostty_vt::Terminal;

const MODES: [&[u8]; 3] = [b"", b"\x1b[>1u", b"\x1b[>31u"];

fn press(stroke: &Keystroke, mac: bool, windows: bool) -> Option<KeyEvent> {
    key_event_for(stroke, KeyAction::Press, mac, windows)
}

fn keystroke(modifiers: Modifiers, key: &str, key_char: Option<&str>) -> Keystroke {
    Keystroke {
        modifiers,
        key: key.into(),
        key_char: key_char.map(str::to_owned),
    }
}

// GPUI names a letter key by its US-position letter on every OS: X11 and
// Wayland through guess_ascii, Mac through the Command layout, Windows through
// the virtual key. Only key_char carries the Cyrillic.
#[test]
fn russian_layout_types_text_and_ctrl_chords_by_physical_key() {
    for (mac, windows) in [(false, false), (true, false), (false, true)] {
        for (key, text) in [("a", "ф"), ("c", "с"), ("q", "й"), ("[", "х")] {
            let plain = keystroke(Modifiers::none(), key, Some(text));
            assert!(press(&plain, mac, windows).is_none(), "{text} is IME text");
        }
        let shifted = keystroke(Modifiers::shift(), "a", Some("Ф"));
        assert!(press(&shifted, mac, windows).is_none());
        for key_char in [None, Some("с")] {
            let ctrl_es = keystroke(Modifiers::control(), "c", key_char);
            let event = press(&ctrl_es, mac, windows).unwrap();
            assert_eq!(event.physical, PhysicalKey::KeyC);
            let mut terminal = Terminal::new(80, 24).unwrap();
            assert_eq!(terminal.encode_key_event(&event).unwrap(), b"\x03");
        }
        let alt_ef = keystroke(Modifiers::alt(), "a", None);
        if !mac {
            let mut terminal = Terminal::new(80, 24).unwrap();
            let event = press(&alt_ef, mac, windows).unwrap();
            assert_eq!(terminal.encode_key_event(&event).unwrap(), b"\x1ba");
        }
    }
}

#[test]
fn a_key_gpui_names_by_its_own_letter_still_has_an_identity() {
    // Windows names OEM keys by the layout's character, e.g. "х" on Russian.
    for name in ["х", "ж", "ü", "ß", "ς", "ש", "ض", "日", "🦊"] {
        let (physical, base) = key_identity(name);
        assert_eq!(physical, PhysicalKey::Unidentified);
        assert!(base.is_some(), "{name}");
        let ctrl = keystroke(Modifiers::control(), name, None);
        let event = press(&ctrl, false, true).unwrap();
        let mut terminal = Terminal::new(80, 24).unwrap();
        let _ = terminal.encode_key_event(&event);
        let release = key_event_for(&ctrl, KeyAction::Release, false, true).unwrap();
        assert_eq!(release.identity(), event.identity());
    }
}

/// Every scalar value as both the key name and its text, under rotating
/// modifiers and OSes, encoded in legacy and kitty modes.
#[test]
fn every_scalar_value_passes_the_key_path_without_panicking() {
    let mut terminals: Vec<Terminal> = MODES
        .iter()
        .map(|mode| {
            let mut terminal = Terminal::new(80, 24).unwrap();
            terminal.feed(mode).unwrap();
            terminal
        })
        .collect();
    let mods = [
        Modifiers::none(),
        Modifiers::shift(),
        Modifiers::control(),
        Modifiers::alt(),
        Modifiers::control_shift(),
        Modifiers {
            control: true,
            alt: true,
            ..Modifiers::none()
        },
        Modifiers::command(),
    ];
    for (n, c) in ('\0'..=char::MAX).enumerate() {
        let name = c.to_string();
        let stroke = keystroke(mods[n % mods.len()], &name, Some(&name));
        let (mac, windows) = (n % 3 == 1, n % 3 == 2);
        for action in [KeyAction::Press, KeyAction::Repeat, KeyAction::Release] {
            if let Some(event) = key_event_for(&stroke, action, mac, windows) {
                let _ = terminals[n % MODES.len()].encode_key_event(&event);
            }
        }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn text(&mut self, len: usize) -> String {
        (0..len)
            .filter_map(|_| char::from_u32((self.next() % 0x11_0000) as u32))
            .collect()
    }
}

#[test]
fn random_keystrokes_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let names = [
        "enter",
        "tab",
        "space",
        "backspace",
        "f1",
        "f25",
        "f99",
        "pageup",
        "kp_enter",
        "cyrillic_ef",
        "dead_acute",
        "",
        "ab",
    ];
    let mut terminal = Terminal::new(80, 24).unwrap();
    for _ in 0..100_000 {
        let key = match rng.below(3) {
            0 => names[rng.below(names.len())].to_owned(),
            _ => {
                let len = 1 + rng.below(2);
                rng.text(len)
            }
        };
        let key_char = match rng.below(2) {
            0 => None,
            _ => {
                let len = rng.below(4);
                Some(rng.text(len))
            }
        };
        let bits = rng.next();
        let modifiers = Modifiers {
            control: bits & 1 != 0,
            alt: bits & 2 != 0,
            shift: bits & 4 != 0,
            platform: bits & 8 != 0,
            function: bits & 16 != 0,
        };
        let stroke = keystroke(modifiers, &key, key_char.as_deref());
        for (mac, windows) in [(false, false), (true, false), (false, true)] {
            if let Some(event) = key_event_for(&stroke, KeyAction::Press, mac, windows) {
                if rng.below(64) == 0 {
                    terminal.feed(MODES[rng.below(MODES.len())]).unwrap();
                }
                let _ = terminal.encode_key_event(&event);
                let _ = terminal.encode_key_event(&event.release());
            }
        }
    }
}
