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
            if let Some(number) = value
                .strip_prefix('f')
                .and_then(|s| s.parse::<u8>().ok())
                .filter(|number| (1..=25).contains(number))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardAction {
    Copy,
    Paste,
}

pub(crate) fn clipboard_shortcut(stroke: &Keystroke) -> Option<ClipboardAction> {
    clipboard_action(stroke, cfg!(target_os = "macos"))
}

fn clipboard_action(stroke: &Keystroke, mac: bool) -> Option<ClipboardAction> {
    let m = stroke.modifiers;
    // The explicit xterm binding is Ctrl+Shift on every platform. On Mac,
    // Chromium also delivers native copy/paste events for Command+C/V.
    let explicit = m.control && m.shift && !m.alt && !m.platform;
    let native_mac = mac && m.platform && !m.control && !m.alt && !m.shift;
    if !explicit && !native_mac {
        return None;
    }
    match stroke.key.as_str() {
        "c" => Some(ClipboardAction::Copy),
        "v" => Some(ClipboardAction::Paste),
        _ => None,
    }
}

pub(crate) fn clipboard_key_bit(key: &str) -> u8 {
    match key {
        "c" => 1,
        "v" => 2,
        _ => 0,
    }
}

pub(crate) fn mac_select_all(stroke: &Keystroke) -> bool {
    let m = stroke.modifiers;
    stroke.key == "a" && m.platform && !m.shift && !m.alt && !m.control
}

pub(crate) fn page_scroll(stroke: &Keystroke, rows: u16) -> Option<isize> {
    if !stroke.modifiers.shift {
        return None;
    }
    let page = rows.saturating_sub(1) as isize;
    match stroke.key.as_str() {
        "pageup" => Some(-page),
        "pagedown" => Some(page),
        _ => None,
    }
}

fn text_commit_expected(stroke: &Keystroke) -> bool {
    let m = stroke.modifiers;
    !m.platform
        && (!m.control && !m.alt
            || cfg!(target_os = "macos") && m.alt && !m.control
            || cfg!(target_os = "windows") && m.alt && m.control && stroke.key != "space")
}

// Printable input goes through GPUI's native committed-text handler, not both paths.
pub fn encode_physical(stroke: &Keystroke, action: KeyAction) -> Option<KeyEvent> {
    let key = physical_key(&stroke.key);
    let modifiers = stroke.modifiers;
    if matches!(key, Key::Character(_) | Key::Unidentified) && text_commit_expected(stroke) {
        return None;
    }
    if key == Key::Unidentified
        || cfg!(target_os = "macos")
            && matches!(key, Key::F(_))
            && modifiers.alt
            && !modifiers.control
            && !modifiers.platform
    {
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
        text: stroke.key_char.clone().filter(|text| {
            !text.is_empty()
                && !text
                    .chars()
                    .any(|c| c.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&c))
        }),
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
