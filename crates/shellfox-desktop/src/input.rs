use gpui::Keystroke;
use shellfox_ghostty_vt::{Key, KeyAction, KeyEvent, Modifiers};

pub fn physical_key(key: &str) -> Key {
    match key {
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "insert" => Key::Insert,
        "delete" => Key::Delete,
        "space" => Key::Character(' '),
        value => {
            if let Some(number) = value.strip_prefix('f').and_then(|s| s.parse::<u8>().ok())
                && (1..=25).contains(&number)
            {
                return Key::F(number);
            }
            let mut chars = value.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Key::Character(ch),
                _ => Key::Unidentified,
            }
        }
    }
}

// Printable input goes through GPUI's native committed-text handler, not both paths.
pub fn encode_physical(stroke: &Keystroke, action: KeyAction) -> Option<KeyEvent> {
    let key = physical_key(&stroke.key);
    let modifiers = stroke.modifiers;
    let modified = modifiers.control || modifiers.alt || modifiers.platform;
    if matches!(key, Key::Character(_) | Key::Unidentified) && !modified {
        return None;
    }
    let unshifted_codepoint = match key {
        Key::Character(ch) => ch as u32,
        _ => 0,
    };
    Some(KeyEvent {
        key,
        action,
        modifiers: Modifiers {
            shift: modifiers.shift,
            control: modifiers.control,
            alt: modifiers.alt,
            super_key: modifiers.platform,
        },
        consumed_modifiers: Modifiers::default(),
        text: None,
        unshifted_codepoint,
        composing: false,
    })
}

pub fn committed_text(text: &str) -> KeyEvent {
    KeyEvent {
        key: Key::Unidentified,
        action: KeyAction::Press,
        modifiers: Modifiers::default(),
        consumed_modifiers: Modifiers::default(),
        text: Some(text.to_owned()),
        unshifted_codepoint: 0,
        composing: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_input_has_only_the_committed_text_path() {
        let stroke = Keystroke::parse("a").unwrap();
        assert!(encode_physical(&stroke, KeyAction::Press).is_none());
        assert_eq!(committed_text("é🦊").text.as_deref(), Some("é🦊"));
    }

    #[test]
    fn ctrl_c_is_terminal_input_not_application_copy() {
        let event =
            encode_physical(&Keystroke::parse("ctrl-c").unwrap(), KeyAction::Press).unwrap();
        assert!(event.modifiers.control);
        assert!(matches!(event.key, Key::Character('c')));
        assert!(event.text.is_none());
    }

    #[test]
    fn arrows_and_repeat_release_are_preserved() {
        let event =
            encode_physical(&Keystroke::parse("shift-up").unwrap(), KeyAction::Release).unwrap();
        assert!(matches!(event.key, Key::ArrowUp));
        assert!(matches!(event.action, KeyAction::Release));
        assert!(event.modifiers.shift);
        assert!(matches!(physical_key("f12"), Key::F(12)));
        assert!(matches!(physical_key("f26"), Key::Unidentified));
    }
}
