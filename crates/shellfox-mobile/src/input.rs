//! Platform input as Ghostty input. The platforms describe what happened (the
//! keyboard's editor text, a key, a paste, a pointer); Ghostty encodes it
//! with the terminal's live modes. Nothing here builds escape sequences.
use crate::composer::Composer;
use crate::engine::{Result, Status};
use serde::Deserialize;
use shellfox_ghostty_vt::key::{KeyAction, KeyEvent};
use shellfox_ghostty_vt::{
    Modifiers, MouseAction, MouseButton, MouseEvent, MouseGeometry, Terminal,
};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Input {
    /// The soft keyboard's committed text after its latest edit. Marked
    /// (composing) text is excluded: it stays a local preedit.
    Editor {
        committed: String,
    },
    /// The platform cleared its keyboard editor; the next text starts fresh.
    EditorReset {},
    Paste {
        text: String,
    },
    Key {
        event: KeyEvent,
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
            Self::Editor { .. } => true,
            Self::EditorReset {} => false,
            Self::Paste { text } => !text.is_empty(),
            Self::Key { event } => event.action != KeyAction::Release,
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
fn encode_error(error: shellfox_ghostty_vt::Error) -> Status {
    match error.0 {
        -2 | -7 => Status::Invalid,
        _ => Status::Engine,
    }
}
pub(crate) fn encode(terminal: &mut Terminal, composer: &mut Composer, input: Input) -> Result<Vec<u8>> {
    match input {
        Input::Editor { committed } => {
            // A keyboard types line breaks and tabs, never other controls.
            let typed = |c: char| matches!(c, '\n' | '\r' | '\t') || !c.is_control();
            if !committed.chars().all(typed) {
                return Err(Status::Invalid);
            }
            let mut bytes = Vec::new();
            for event in composer.update(&committed) {
                bytes.extend(terminal.encode_key_event(&event).map_err(encode_error)?);
            }
            Ok(bytes)
        }
        Input::EditorReset {} => {
            composer.reset();
            Ok(Vec::new())
        }
        Input::Paste { text } => {
            composer.detach();
            terminal.encode_paste(&text).map_err(encode_error)
        }
        Input::Key { event } => {
            event.validate().map_err(|_| Status::Invalid)?;
            composer.detach();
            terminal.encode_key_event(&event).map_err(encode_error)
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
    fn run(terminal: &mut Terminal, composer: &mut Composer, json: &str) -> Result<Vec<u8>> {
        let input = serde_json::from_str::<Input>(json).map_err(|_| Status::Invalid)?;
        encode(terminal, composer, input)
    }
    #[test]
    fn input_rejects_unknown_fields_and_variants() {
        for value in [
            r#"{"type":"escape","text":"bad"}"#,
            r#"{"type":"editor","committed":"a","hostSecret":"bad"}"#,
            r#"{"type":"commit","text":"a"}"#,
            r#"{"type":"key","event":{"physical":999,"action":"press","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false}}"#,
        ] {
            assert!(serde_json::from_str::<Input>(value).is_err(), "{value}");
        }
    }
    #[test]
    fn editor_text_key_paste_and_mouse_use_terminal_modes() {
        let mut terminal = Terminal::new(20, 5).unwrap();
        let mut composer = Composer::default();
        terminal
            .feed(b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h")
            .unwrap();
        let t = &mut terminal;
        let c = &mut composer;
        assert_eq!(run(t, c, r#"{"type":"editor","committed":"日本"}"#).unwrap(), "日本".as_bytes());
        assert_eq!(run(t, c, r#"{"type":"editor","committed":"日"}"#).unwrap(), b"\x7f");
        assert!(run(t, c, r#"{"type":"editorReset"}"#).unwrap().is_empty());
        assert_eq!(run(t, c, r#"{"type":"editor","committed":"x"}"#).unwrap(), b"x");
        let up = r#"{"type":"key","event":{"physical":78,"action":"press","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false}}"#;
        assert_eq!(run(t, c, up).unwrap(), b"\x1bOA");
        let ctrl_c = r#"{"type":"key","event":{"physical":22,"action":"press","modifiers":2,"consumedModifiers":0,"text":"c","unshiftedCodepoint":99,"composing":false}}"#;
        assert_eq!(run(t, c, ctrl_c).unwrap(), b"\x03");
        assert_eq!(
            run(t, c, r#"{"type":"paste","text":"x\ny"}"#).unwrap(),
            b"\x1b[200~x\ny\x1b[201~"
        );
        let mouse = r#"{"type":"mouse","action":"press","button":"left","x":1,"y":1,"cellWidth":8,"cellHeight":16,"anyButtonPressed":true}"#;
        assert_eq!(run(t, c, mouse).unwrap(), b"\x1b[<0;1;1M");
    }
}
