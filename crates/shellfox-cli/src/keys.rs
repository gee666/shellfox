use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, PartialEq)]
pub enum Input {
    Detach,
    Bytes(Vec<u8>),
    Ignore,
}

// Never classify response-looking user text as an automatic terminal reply.
// Queries are suppressed on the output path, not guessed from ambiguous input.
pub fn from_event(event: Event) -> Input {
    match event {
        Event::Key(key) => encode(key),
        Event::Paste(text) => Input::Bytes(text.into_bytes()),
        _ => Input::Ignore,
    }
}

pub fn encode(key: KeyEvent) -> Input {
    if key.kind == KeyEventKind::Release {
        return Input::Ignore;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if key.code == KeyCode::Char('\u{1d}') || (ctrl && key.code == KeyCode::Char(']')) {
        return Input::Detach;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META)
    {
        return Input::Ignore;
    }
    let modifier = 1
        + u8::from(key.modifiers.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(alt)
        + 4 * u8::from(ctrl);
    let mut bytes = match key.code {
        KeyCode::Char(c) if ctrl => {
            let code = match c.to_ascii_uppercase() {
                '@' | ' ' | '2' => 0,
                c @ 'A'..='Z' => c as u8 - b'A' + 1,
                '[' | '3' => 27,
                '\\' | '4' => 28,
                '^' | '6' => 30,
                '_' | '7' | '/' => 31,
                '?' | '8' => 127,
                _ => return Input::Ignore,
            };
            vec![code]
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => return Input::Bytes(b"\x1b[Z".to_vec()),
        KeyCode::Backspace => vec![127],
        KeyCode::Esc => vec![27],
        KeyCode::Up => return csi('A', modifier),
        KeyCode::Down => return csi('B', modifier),
        KeyCode::Right => return csi('C', modifier),
        KeyCode::Left => return csi('D', modifier),
        KeyCode::Home => return csi('H', modifier),
        KeyCode::End => return csi('F', modifier),
        KeyCode::Insert => return tilde(2, modifier),
        KeyCode::Delete => return tilde(3, modifier),
        KeyCode::PageUp => return tilde(5, modifier),
        KeyCode::PageDown => return tilde(6, modifier),
        KeyCode::F(n @ 1..=4) => {
            let final_char = char::from(b'P' + n - 1);
            return if modifier == 1 {
                Input::Bytes(format!("\x1bO{final_char}").into_bytes())
            } else {
                csi(final_char, modifier)
            };
        }
        KeyCode::F(n @ 5..=12) => {
            return tilde(
                [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)],
                modifier,
            )
        }
        _ => return Input::Ignore,
    };
    if alt {
        bytes.insert(0, 27);
    }
    Input::Bytes(bytes)
}

fn csi(final_char: char, modifier: u8) -> Input {
    Input::Bytes(
        if modifier == 1 {
            format!("\x1b[{final_char}")
        } else {
            format!("\x1b[1;{modifier}{final_char}")
        }
        .into_bytes(),
    )
}

fn tilde(code: u8, modifier: u8) -> Input {
    Input::Bytes(
        if modifier == 1 {
            format!("\x1b[{code}~")
        } else {
            format!("\x1b[{code};{modifier}~")
        }
        .into_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode, modifiers: KeyModifiers) -> Input {
        encode(KeyEvent::new(code, modifiers))
    }
    #[test]
    fn detach_is_local_but_ctrl_c_is_remote_input() {
        assert_eq!(
            key(KeyCode::Char(']'), KeyModifiers::CONTROL),
            Input::Detach
        );
        assert_eq!(
            key(KeyCode::Char('\u{1d}'), KeyModifiers::NONE),
            Input::Detach
        );
        assert_eq!(
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Input::Bytes(vec![3])
        );
        assert_eq!(
            key(KeyCode::Char(']'), KeyModifiers::NONE),
            Input::Bytes(vec![b']'])
        );
    }
    #[test]
    fn unicode_alt_and_navigation_keys() {
        assert_eq!(
            key(KeyCode::Char('界'), KeyModifiers::NONE),
            Input::Bytes("界".as_bytes().to_vec())
        );
        assert_eq!(
            key(KeyCode::Char('x'), KeyModifiers::ALT),
            Input::Bytes(b"\x1bx".to_vec())
        );
        assert_eq!(
            key(KeyCode::Up, KeyModifiers::NONE),
            Input::Bytes(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key(KeyCode::Left, KeyModifiers::CONTROL),
            Input::Bytes(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            key(KeyCode::F(5), KeyModifiers::NONE),
            Input::Bytes(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            key(KeyCode::BackTab, KeyModifiers::SHIFT),
            Input::Bytes(b"\x1b[Z".to_vec())
        );
    }
    #[test]
    fn response_looking_keys_and_bracketed_paste_are_not_stripped() {
        let replies = "\x1b[?61;6;7;22c\x1b[12;34R\x1b[4;900;1200t\x1b]10;rgb:ff/ff/ff\x1b\\";
        let input: Vec<u8> = replies
            .chars()
            .flat_map(|c| {
                match from_event(Event::Key(KeyEvent::new(
                    KeyCode::Char(c),
                    KeyModifiers::NONE,
                ))) {
                    Input::Bytes(bytes) => bytes,
                    _ => panic!("User key was classified as a reply"),
                }
            })
            .collect();
        assert_eq!(input, replies.as_bytes());
        // Event::Paste is already framed by crossterm. Its payload is opaque,
        // including literal bracket delimiters and the local detach character.
        let paste = format!("\x1b[200~{replies}\x1d\n雪🙂\x1b[201~");
        assert_eq!(
            from_event(Event::Paste(paste.clone())),
            Input::Bytes(paste.into_bytes())
        );
    }

    #[test]
    fn release_is_not_repeated_input() {
        let event = KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(encode(event), Input::Ignore);
        assert_eq!(key(KeyCode::Null, KeyModifiers::NONE), Input::Ignore);
    }
}
