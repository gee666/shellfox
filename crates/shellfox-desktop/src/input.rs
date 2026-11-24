use gpui::Keystroke;
use shellfox_ghostty_vt::key::{
    KeyAction, KeyEvent, KeyModifiers, PhysicalKey, us_key, validate_text,
};

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
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
                (Some(c), None) => us_key(c).map_or(P::Unidentified, |(key, _, _)| key),
                _ => P::Unidentified,
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
/// type them twice. Mac Option that composes nothing is an Alt chord, as in 0.9.
fn text_commit_expected(stroke: &Keystroke, mac: bool, windows: bool) -> bool {
    let m = stroke.modifiers;
    let single = stroke.key.chars().count() == 1 || stroke.key == "space";
    single
        && !m.platform
        && (!m.control && !m.alt
            || mac && m.alt && !m.control && stroke.key_char.is_some()
            || windows && m.alt && m.control && stroke.key_char.is_some())
}

/// A GPUI keystroke as a Ghostty key event. Printable text without Ctrl/Alt
/// comes through `KeyEvent::text` from the input handler instead.
pub fn key_event(stroke: &Keystroke, action: KeyAction) -> Option<KeyEvent> {
    key_event_for(
        stroke,
        action,
        cfg!(target_os = "macos"),
        cfg!(target_os = "windows"),
    )
}

fn key_event_for(
    stroke: &Keystroke,
    action: KeyAction,
    mac: bool,
    windows: bool,
) -> Option<KeyEvent> {
    if text_commit_expected(stroke, mac, windows) {
        return None;
    }
    let (physical, base) = key_identity(&stroke.key);
    if physical == PhysicalKey::Unidentified && base.is_none() {
        return None;
    }
    let m = stroke.modifiers;
    let character = single_char(&stroke.key);
    // GPUI folds Shift into shifted symbols ("!" without Shift); Shift is back.
    let folded_shift = character
        .and_then(us_key)
        .is_some_and(|(_, b, s)| s && Some(b) != character);
    let mut modifiers = KeyModifiers::NONE;
    modifiers.set(KeyModifiers::SHIFT, m.shift || folded_shift);
    modifiers.set(KeyModifiers::CONTROL, m.control);
    modifiers.set(KeyModifiers::ALT, m.alt);
    modifiers.set(KeyModifiers::SUPER, m.platform);
    // Layout text without Ctrl. GPUI omits key_char under Ctrl, so rebuild
    // it from the key name, uppercasing letters under Shift.
    let text = match action {
        KeyAction::Release => String::new(),
        _ => stroke
            .key_char
            .clone()
            .filter(|text| validate_text(text).is_ok())
            .or_else(|| {
                let c = if stroke.key == "space" {
                    ' '
                } else {
                    character?
                };
                Some(if m.shift {
                    c.to_uppercase().collect()
                } else {
                    c.to_string()
                })
            })
            .unwrap_or_default(),
    };
    let consumed_modifiers = match base {
        Some(b)
            if modifiers.contains(KeyModifiers::SHIFT)
                && !text.is_empty()
                && text.chars().ne([b]) =>
        {
            KeyModifiers::SHIFT
        }
        _ => KeyModifiers::NONE,
    };
    Some(KeyEvent {
        physical,
        action,
        modifiers,
        consumed_modifiers,
        text,
        unshifted_codepoint: base,
        composing: false,
    })
}

fn single_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    chars.next().filter(|_| chars.next().is_none())
}

/// Ghostty's key and the key's unshifted character. GPUI names printable
/// keys by their character, and a shifted symbol names the same key as its
/// base, so press and release match even when Shift is let go first.
pub fn key_identity(key: &str) -> (PhysicalKey, Option<char>) {
    if key == "space" {
        return (PhysicalKey::Space, Some(' '));
    }
    match single_char(key) {
        Some(c) => match us_key(c) {
            Some((physical, base, _)) => (physical, Some(base)),
            None => (
                PhysicalKey::Unidentified,
                Some(c.to_lowercase().next().unwrap_or(c)),
            ),
        },
        None => (physical_key(key), None),
    }
}
