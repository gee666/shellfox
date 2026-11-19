//! Keyboard bytes produced by xterm.js 6's Keyboard.ts. Ghostty still owns
//! terminal state, including DECCKM; its native encoder remains available to
//! clients that send Ghostty key events rather than xterm-compatible input.
use crate::{Key, KeyAction, KeyEvent, Modifiers, Result, Terminal};
use shellfox_ghostty_vt_sys::XtermInputMode;

impl Terminal {
    pub fn encode_xterm_key(&self, event: &KeyEvent) -> Result<Vec<u8>> {
        event.native()?;
        let m = event.modifiers;
        let application = m.bits() == 0
            && matches!(
                event.key,
                Key::ArrowUp
                    | Key::ArrowDown
                    | Key::ArrowLeft
                    | Key::ArrowRight
                    | Key::Home
                    | Key::End
            )
            && self.xterm_mode(XtermInputMode::ApplicationCursorKeys)?;
        Ok(encode(event, application))
    }

    pub fn encode_xterm_paste(&self, text: &str) -> Result<Vec<u8>> {
        let bracketed = self.xterm_mode(XtermInputMode::BracketedPaste)?;
        let mut result =
            Vec::with_capacity(text.len().saturating_add(if bracketed { 12 } else { 0 }));
        if bracketed {
            result.extend_from_slice(b"\x1b[200~");
        }
        let mut bytes = text.bytes().peekable();
        while let Some(byte) = bytes.next() {
            if byte == b'\r' && bytes.peek() == Some(&b'\n') {
                bytes.next();
            }
            result.push(if byte == b'\n' { b'\r' } else { byte });
        }
        if bracketed {
            result.extend_from_slice(b"\x1b[201~");
        }
        Ok(result)
    }

    pub fn encode_xterm_focus(&self, focused: bool) -> Result<Option<&'static [u8]>> {
        Ok(self
            .xterm_mode(XtermInputMode::FocusReporting)?
            .then_some(if focused { b"\x1b[I" } else { b"\x1b[O" }))
    }

    fn xterm_mode(&self, mode: XtermInputMode) -> Result<bool> {
        self.engine.xterm_input_mode(mode).map_err(crate::Error)
    }
}

fn encode(event: &KeyEvent, application: bool) -> Vec<u8> {
    if event.action == KeyAction::Release || event.composing {
        return Vec::new();
    }
    let m = event.modifiers;
    let parameter = 1
        + u8::from(m.shift)
        + 2 * u8::from(m.alt)
        + 4 * u8::from(m.control)
        + 8 * u8::from(m.super_key);
    let prefixed = |key| if m.alt { vec![27, key] } else { vec![key] };
    let csi = |key| format!("\x1b[1;{parameter}{key}").into_bytes();
    let letter = |key| match (parameter, application) {
        (1, false) => vec![27, b'[', key],
        (1, true) => vec![27, b'O', key],
        _ => csi(char::from(key)),
    };
    let tilde = |code| {
        if parameter == 1 {
            format!("\x1b[{code}~").into_bytes()
        } else {
            format!("\x1b[{code};{parameter}~").into_bytes()
        }
    };
    match event.key {
        Key::Enter => prefixed(b'\r'),
        Key::Escape => prefixed(27),
        Key::Backspace => prefixed(if m.control { 8 } else { 127 }),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight if m.super_key => {
            Vec::new()
        }
        Key::ArrowUp => letter(b'A'),
        Key::ArrowDown => letter(b'B'),
        Key::ArrowRight => letter(b'C'),
        Key::ArrowLeft => letter(b'D'),
        Key::Home => letter(b'H'),
        Key::End => letter(b'F'),
        Key::Insert if m.shift || m.control => Vec::new(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::Delete => tilde(3),
        Key::PageUp | Key::PageDown if m.shift => Vec::new(), // scrolls the view
        Key::PageUp if m.control => tilde(5),
        Key::PageDown if m.control => tilde(6),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::F(n @ 1..=4) if parameter == 1 => vec![27, b'O', b'P' + n - 1],
        Key::F(n @ 1..=4) => csi(char::from(b'P' + n - 1)),
        Key::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        Key::F(_) => Vec::new(),
        Key::Character(key) => character(key, event.text.as_deref(), m),
        Key::Unidentified => event
            .text
            .as_deref()
            .unwrap_or_default()
            .as_bytes()
            .to_vec(),
    }
}

const UNSHIFTED: &str = "1234567890;=,-./`[\\]'";
const SHIFTED: &str = "!@#$%^&*():+<_>?~{|}\"";

fn character(key: char, text: Option<&str>, mut m: Modifiers) -> Vec<u8> {
    // GPUI on Windows folds Shift+digit/OEM into the shifted symbol and clears
    // the Shift bit. xterm expects the key and its modifier separately.
    let key = if let Some(index) = SHIFTED.find(key) {
        m.shift = true;
        char::from(UNSHIFTED.as_bytes()[index])
    } else {
        key
    };
    let printable = || {
        text.map(str::to_owned)
            .unwrap_or_else(|| if m.shift { shifted(key) } else { key }.to_string())
    };
    let key = key.to_ascii_lowercase();
    let control_letter = || key.is_ascii_lowercase().then(|| key as u8 - b'a' + 1);
    if m.control && !m.shift && !m.alt && !m.super_key {
        return control_letter()
            .or_else(|| match key {
                ' ' => Some(0),
                '3'..='7' => Some(key as u8 - b'3' + 27),
                '8' => Some(127),
                '[' => Some(27),
                '\\' => Some(28),
                ']' => Some(29),
                _ => None,
            })
            .into_iter()
            .collect();
    }
    if m.alt && !m.super_key {
        let second = match key {
            ' ' if m.control => Some(0),
            'a'..='z' if m.control => control_letter(),
            'a'..='z'
            | '0'..='9'
            | ';'
            | '='
            | ','
            | '-'
            | '.'
            | '/'
            | '`'
            | '['
            | '\\'
            | ']'
            | '\''
            | ' ' => Some(if m.shift { shifted(key) } else { key } as u8),
            _ => None,
        };
        return second.map_or_else(Vec::new, |byte| vec![27, byte]);
    }
    if m.control {
        return match printable().as_str() {
            "_" => vec![31],
            "@" => vec![0],
            _ => Vec::new(),
        };
    }
    if m.super_key {
        Vec::new()
    } else {
        printable().into_bytes()
    }
}

fn shifted(key: char) -> char {
    UNSHIFTED.find(key).map_or_else(
        || key.to_ascii_uppercase(),
        |index| char::from(SHIFTED.as_bytes()[index]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_and_alt_sequences_use_xterm_not_ghostty_defaults() {
        let ctrl = Modifiers {
            control: true,
            ..Default::default()
        };
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        let ctrl_shift = Modifiers {
            shift: true,
            ..ctrl
        };
        let alt_shift = Modifiers { shift: true, ..alt };
        for (key, modifiers, bytes) in [
            (Key::Character('c'), ctrl, b"\x03".as_slice()),
            (Key::Character('v'), ctrl, b"\x16"),
            (Key::Character(' '), ctrl, b"\0"),
            (Key::Character('2'), ctrl, b""),
            (Key::Character('2'), ctrl_shift, b"\0"),
            (Key::Character('-'), ctrl_shift, b"\x1f"),
            (Key::Character('/'), ctrl, b""),
            (Key::Character('a'), alt_shift, b"\x1bA"),
            (Key::Character('4'), alt_shift, b"\x1b$"),
            (Key::Enter, ctrl, b"\r"),
            (Key::Enter, alt, b"\x1b\r"),
            (Key::Tab, alt, b"\t"),
            (Key::Tab, ctrl_shift, b"\x1b[Z"),
            (Key::Escape, shift, b"\x1b"),
            (Key::Backspace, ctrl, b"\x08"),
            (Key::Insert, shift, b""),
            (Key::Insert, alt, b"\x1b[2~"),
            (Key::PageDown, alt, b"\x1b[6~"),
            (Key::PageDown, ctrl, b"\x1b[6;5~"),
            (Key::PageDown, ctrl_shift, b""),
            (Key::F(3), ctrl, b"\x1b[1;5R"),
            (Key::F(12), shift, b"\x1b[24;2~"),
            (Key::F(13), shift, b""),
        ] {
            let event = KeyEvent {
                key,
                modifiers,
                ..Default::default()
            };
            assert_eq!(encode(&event, false), bytes, "{key:?} {modifiers:?}");
            assert!(encode(
                &KeyEvent {
                    action: KeyAction::Release,
                    ..event
                },
                false
            )
            .is_empty());
        }
    }

    #[test]
    fn alt_with_all_shifted_ascii_keys_accepts_a_folded_symbol() {
        for (plain, shifted) in UNSHIFTED
            .bytes()
            .zip(SHIFTED.bytes())
            .chain((b'a'..=b'z').zip(b'A'..=b'Z'))
        {
            let alt = Modifiers {
                alt: true,
                ..Default::default()
            };
            let with_shift = KeyEvent {
                key: Key::Character(plain as char),
                modifiers: Modifiers { shift: true, ..alt },
                ..Default::default()
            };
            assert_eq!(encode(&with_shift, false), [27, shifted]);
            if plain.is_ascii_lowercase() {
                continue;
            }
            let folded = KeyEvent {
                key: Key::Character(shifted as char),
                modifiers: alt,
                ..Default::default()
            };
            assert_eq!(encode(&folded, false), [27, shifted]);
        }
    }

    #[test]
    fn paste_and_focus_follow_live_dec_modes() {
        let mut term = Terminal::new(80, 24).unwrap();
        assert_eq!(
            term.encode_xterm_paste("a\r\nb\nc\r").unwrap(),
            b"a\rb\rc\r"
        );
        assert_eq!(term.encode_xterm_focus(true).unwrap(), None);
        term.feed(b"\x1b[?2004h\x1b[?1004h").unwrap();
        assert_eq!(
            term.encode_xterm_paste("a\r\nb\nc").unwrap(),
            b"\x1b[200~a\rb\rc\x1b[201~"
        );
        assert_eq!(
            term.encode_xterm_focus(true).unwrap(),
            Some(b"\x1b[I".as_slice())
        );
        assert_eq!(
            term.encode_xterm_focus(false).unwrap(),
            Some(b"\x1b[O".as_slice())
        );
        term.feed(b"\x1b[?1004l\x1b[?2004l").unwrap();
        assert_eq!(term.encode_xterm_paste("a\nb").unwrap(), b"a\rb");
        assert_eq!(term.encode_xterm_focus(false).unwrap(), None);
    }

    #[test]
    fn cursor_modes_come_from_the_terminal_and_committed_text_is_utf8() {
        let mut term = Terminal::new(80, 24).unwrap();
        let up = KeyEvent {
            key: Key::ArrowUp,
            ..Default::default()
        };
        assert_eq!(term.encode_xterm_key(&up).unwrap(), b"\x1b[A");
        term.feed(b"\x1b[?1h").unwrap();
        assert_eq!(term.encode_xterm_key(&up).unwrap(), b"\x1bOA");
        let shift_up = KeyEvent {
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            ..up
        };
        assert_eq!(term.encode_xterm_key(&shift_up).unwrap(), b"\x1b[1;2A");
        let commit = KeyEvent {
            text: Some("ø🦊".into()),
            ..Default::default()
        };
        assert_eq!(term.encode_xterm_key(&commit).unwrap(), "ø🦊".as_bytes());
    }
}
