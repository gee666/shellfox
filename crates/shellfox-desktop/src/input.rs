use gpui::Keystroke;
use shellfox_ghostty_vt::key::{KeyAction, KeyEvent, KeyModifiers, PhysicalKey};

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

/// GPUI key name to Ghostty's key. GPUI reports layout keys, not scan codes,
/// so a single character names the key at that US position when it has one.
pub fn physical_key(key: &str) -> PhysicalKey {
    use PhysicalKey as P;
    match key {
        "enter" => P::Enter,
        "tab" => P::Tab,
        "backspace" => P::Backspace,
        "escape" => P::Escape,
        "up" => P::ArrowUp,
        "down" => P::ArrowDown,
        "left" => P::ArrowLeft,
        "right" => P::ArrowRight,
        "home" => P::Home,
        "end" => P::End,
        "pageup" => P::PageUp,
        "pagedown" => P::PageDown,
        "insert" => P::Insert,
        "delete" => P::Delete,
        "space" => P::Space,
        "menu" => P::ContextMenu,
        value => {
            if let Some(n) = value.strip_prefix('f').and_then(|s| s.parse::<u32>().ok()) {
                return match n {
                    1..=25 => P::try_from(P::F1.code() + n - 1).unwrap_or(P::Unidentified),
                    _ => P::Unidentified,
                };
            }
            let mut chars = value.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => character_key(c),
                _ => P::Unidentified,
            }
        }
    }
}

fn character_key(c: char) -> PhysicalKey {
    use PhysicalKey as P;
    let offset = |base: char| u32::from(c) - u32::from(base);
    match c {
        'a'..='z' => P::try_from(P::KeyA.code() + offset('a')).unwrap_or(P::Unidentified),
        '0'..='9' => P::try_from(P::Digit0.code() + offset('0')).unwrap_or(P::Unidentified),
        '`' => P::Backquote,
        '-' => P::Minus,
        '=' => P::Equal,
        '[' => P::BracketLeft,
        ']' => P::BracketRight,
        '\\' => P::Backslash,
        ';' => P::Semicolon,
        '\'' => P::Quote,
        ',' => P::Comma,
        '.' => P::Period,
        '/' => P::Slash,
        _ => P::Unidentified,
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
    // Ctrl+Shift+C/V on every platform, plus Command+C/V on Mac.
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

/// Keys whose text the platform delivers through the input handler (IME,
/// dead keys, Option on Mac, AltGr on Windows). Sending them here too would
/// type them twice.
fn text_commit_expected(stroke: &Keystroke, mac: bool, windows: bool) -> bool {
    let m = stroke.modifiers;
    let single = stroke.key.chars().count() == 1 || stroke.key == "space";
    single
        && !m.platform
        && (!m.control && !m.alt
            || mac && m.alt && !m.control
            || windows && m.alt && m.control && stroke.key_char.is_some())
}

/// A GPUI keystroke as a Ghostty key event. Printable text without Ctrl/Alt
/// comes through `KeyEvent::text` from the input handler instead.
pub fn key_event(stroke: &Keystroke, action: KeyAction) -> Option<KeyEvent> {
    key_event_for(stroke, action, cfg!(target_os = "macos"), cfg!(target_os = "windows"))
}

fn key_event_for(stroke: &Keystroke, action: KeyAction, mac: bool, windows: bool) -> Option<KeyEvent> {
    if text_commit_expected(stroke, mac, windows) {
        return None;
    }
    let (physical, base) = key_identity(&stroke.key);
    let m = stroke.modifiers;
    let mut modifiers = KeyModifiers::NONE;
    modifiers.set(KeyModifiers::SHIFT, m.shift);
    modifiers.set(KeyModifiers::CONTROL, m.control);
    modifiers.set(KeyModifiers::ALT, m.alt);
    modifiers.set(KeyModifiers::SUPER, m.platform);
    if physical == PhysicalKey::Unidentified && base.is_none() {
        return None;
    }
    // Layout text without Ctrl. GPUI omits key_char under Ctrl, so rebuild
    // it from the base character; Shift only changes letters here because
    // GPUI already folds Shift into shifted symbols.
    let text = match action {
        KeyAction::Release => String::new(),
        _ => stroke
            .key_char
            .clone()
            .filter(|text| shellfox_ghostty_vt::key::validate_text(text).is_ok())
            .or_else(|| {
                base.map(|c| match m.shift {
                    true => c.to_uppercase().collect(),
                    false => c.to_string(),
                })
            })
            .unwrap_or_default(),
    };
    let consumed_modifiers = match base {
        Some(c) if m.shift && text.chars().ne(c.to_lowercase()) => KeyModifiers::SHIFT,
        _ => KeyModifiers::NONE,
    };
    Some(KeyEvent {
        physical,
        action,
        modifiers,
        consumed_modifiers,
        text,
        unshifted_codepoint: base.map(|c| c.to_lowercase().next().unwrap_or(c)),
        composing: false,
    })
}

/// Ghostty's key and the key's base character. GPUI names printable keys by
/// their character, so this survives modifier changes between press and release.
pub fn key_identity(key: &str) -> (PhysicalKey, Option<char>) {
    let base = match key {
        "space" => Some(' '),
        key => {
            let mut chars = key.chars();
            chars.next().filter(|_| chars.next().is_none())
        }
    };
    (physical_key(key), base)
}
