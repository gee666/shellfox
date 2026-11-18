use crate::engine::{Result, Status};
use serde::Deserialize;
use shellfox_ghostty_vt::{
    Key, KeyAction, KeyEvent, Modifiers, MouseAction, MouseButton, MouseEvent, MouseGeometry,
    Terminal,
};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Input {
    Commit {
        text: String,
    },
    Preedit {
        text: String,
    },
    Paste {
        text: String,
    },
    Key {
        key: PhysicalKey,
        action: Action,
        #[serde(default)]
        modifiers: u8,
        #[serde(default, rename = "consumedModifiers")]
        consumed_modifiers: u8,
        #[serde(default)]
        text: Option<String>,
        #[serde(default, rename = "unshiftedCodepoint")]
        unshifted_codepoint: u32,
    },
    Mouse {
        action: PointerAction,
        button: Option<Button>,
        #[serde(default)]
        modifiers: u8,
        x: f32,
        y: f32,
        #[serde(rename = "cellWidth")]
        cell_width: u32,
        #[serde(rename = "cellHeight")]
        cell_height: u32,
        #[serde(rename = "anyButtonPressed")]
        any_button_pressed: bool,
    },
}
impl Input {
    /// Gesture intent only. The host still filters passive encoded reports.
    pub(crate) fn user_initiated(&self) -> bool {
        match self {
            Self::Commit { text } | Self::Paste { text } => !text.is_empty(),
            Self::Preedit { .. }
            | Self::Key {
                action: Action::Release,
                ..
            } => false,
            Self::Key { .. } => true,
            Self::Mouse {
                action: PointerAction::Motion,
                any_button_pressed,
                ..
            } => *any_button_pressed,
            Self::Mouse { .. } => true,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Action {
    Press,
    Repeat,
    Release,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PointerAction {
    Press,
    Release,
    Motion,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    Back,
    Forward,
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum PhysicalKey {
    Unidentified,
    Character(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Function(u8),
}
fn modifiers(bits: u8) -> Result<Modifiers> {
    if bits > 15 {
        return Err(Status::Invalid);
    }
    Ok(Modifiers {
        shift: bits & 1 != 0,
        control: bits & 2 != 0,
        alt: bits & 4 != 0,
        super_key: bits & 8 != 0,
    })
}
impl PhysicalKey {
    fn key(self) -> Result<Key> {
        Ok(match self {
            Self::Unidentified => Key::Unidentified,
            Self::Character(c) if c.is_ascii() && !c.is_ascii_control() => Key::Character(c),
            Self::Character(_) => return Err(Status::Invalid),
            Self::Function(n) if (1..=25).contains(&n) => Key::F(n),
            Self::Function(_) => return Err(Status::Invalid),
            Self::Enter => Key::Enter,
            Self::Tab => Key::Tab,
            Self::Backspace => Key::Backspace,
            Self::Escape => Key::Escape,
            Self::ArrowUp => Key::ArrowUp,
            Self::ArrowDown => Key::ArrowDown,
            Self::ArrowLeft => Key::ArrowLeft,
            Self::ArrowRight => Key::ArrowRight,
            Self::Home => Key::Home,
            Self::End => Key::End,
            Self::PageUp => Key::PageUp,
            Self::PageDown => Key::PageDown,
            Self::Insert => Key::Insert,
            Self::Delete => Key::Delete,
        })
    }
}
fn encode_error(error: shellfox_ghostty_vt::Error) -> Status {
    match error.0 {
        -2 | -7 => Status::Invalid,
        _ => Status::Engine,
    }
}
pub(crate) fn encode(terminal: &mut Terminal, input: Input) -> Result<Vec<u8>> {
    match input {
        Input::Preedit { text } => {
            if text.chars().any(char::is_control) {
                return Err(Status::Invalid);
            }
            Ok(Vec::new())
        }
        Input::Commit { text } => terminal
            .encode_key(&KeyEvent {
                text: Some(text),
                ..Default::default()
            })
            .map_err(encode_error),
        Input::Paste { text } => terminal.encode_paste(&text).map_err(encode_error),
        Input::Key {
            key,
            action,
            modifiers: bits,
            consumed_modifiers,
            text,
            unshifted_codepoint,
        } => {
            let event = KeyEvent {
                key: key.key()?,
                action: match action {
                    Action::Press => KeyAction::Press,
                    Action::Release => KeyAction::Release,
                    Action::Repeat => KeyAction::Repeat,
                },
                modifiers: modifiers(bits)?,
                consumed_modifiers: modifiers(consumed_modifiers)?,
                text,
                unshifted_codepoint,
                composing: false,
            };
            terminal.encode_key(&event).map_err(encode_error)
        }
        Input::Mouse {
            action,
            button,
            modifiers: bits,
            x,
            y,
            cell_width,
            cell_height,
            any_button_pressed,
        } => {
            let button = button.map(|b| match b {
                Button::Left => MouseButton::Left,
                Button::Middle => MouseButton::Middle,
                Button::Right => MouseButton::Right,
                Button::WheelUp => MouseButton::WheelUp,
                Button::WheelDown => MouseButton::WheelDown,
                Button::WheelLeft => MouseButton::WheelLeft,
                Button::WheelRight => MouseButton::WheelRight,
                Button::Back => MouseButton::Back,
                Button::Forward => MouseButton::Forward,
            });
            terminal
                .encode_mouse(
                    &MouseEvent {
                        action: match action {
                            PointerAction::Press => MouseAction::Press,
                            PointerAction::Release => MouseAction::Release,
                            PointerAction::Motion => MouseAction::Motion,
                        },
                        button,
                        modifiers: modifiers(bits)?,
                        x,
                        y,
                        any_button_pressed,
                    },
                    MouseGeometry {
                        cell_width,
                        cell_height,
                    },
                )
                .map_err(encode_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_rejects_unknown_fields_and_variants() {
        for value in [
            r#"{"type":"escape","text":"bad"}"#,
            r#"{"type":"commit","text":"a","hostSecret":"bad"}"#,
            r#"{"type":"key","key":{"kind":"enter","value":4},"action":"press"}"#,
        ] {
            assert!(serde_json::from_str::<Input>(value).is_err(), "{value}");
        }
    }
    #[test]
    fn preedit_does_not_commit_and_unicode_commit_uses_encoder() {
        let mut terminal = Terminal::new(20, 5).unwrap();
        assert!(encode(
            &mut terminal,
            Input::Preedit {
                text: "日本".into()
            }
        )
        .unwrap()
        .is_empty());
        assert_eq!(
            encode(
                &mut terminal,
                Input::Commit {
                    text: "日本".into()
                }
            )
            .unwrap(),
            "日本".as_bytes()
        );
        assert_eq!(
            encode(
                &mut terminal,
                Input::Commit {
                    text: "\u{1b}[A".into()
                }
            ),
            Err(Status::Invalid)
        );
    }
    #[test]
    fn uses_terminal_key_paste_and_mouse_modes() {
        let mut terminal = Terminal::new(20, 5).unwrap();
        terminal
            .feed(b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h")
            .unwrap();
        let key = serde_json::from_str::<Input>(
            r#"{"type":"key","key":{"kind":"arrowUp"},"action":"press"}"#,
        )
        .unwrap();
        assert_eq!(encode(&mut terminal, key).unwrap(), b"\x1bOA");
        assert_eq!(
            encode(
                &mut terminal,
                Input::Paste {
                    text: "x\ny".into()
                }
            )
            .unwrap(),
            b"\x1b[200~x\ny\x1b[201~"
        );
        let mouse = serde_json::from_str::<Input>(
            r#"{"type":"mouse","action":"press","button":"left","x":1,"y":1,"cellWidth":8,"cellHeight":16,"anyButtonPressed":true}"#).unwrap();
        assert_eq!(encode(&mut terminal, mouse).unwrap(), b"\x1b[<0;1;1M");
    }
}
