//! Crossterm events as Ghostty key events. A Ghostty terminal that knows the
//! application's modes encodes them; nothing here produces terminal bytes.
use crossterm::event::{Event, KeyCode, KeyEvent as Crossterm, KeyEventKind, KeyModifiers};
use shellfox_ghostty_vt::key::{KeyAction, KeyEvent, KeyModifiers as Mods, PhysicalKey as P};

#[derive(Debug, PartialEq)]
pub enum Input {
    Detach,
    Key(KeyEvent),
    Paste(String),
    Ignore,
}

// Never classify response-looking user text as an automatic terminal reply.
// Queries are suppressed on the output path, not guessed from ambiguous input.
pub fn from_event(event: Event) -> Input {
    match event {
        Event::Key(key) => key_input(key),
        Event::Paste(text) => Input::Paste(text),
        _ => Input::Ignore,
    }
}

pub fn key_input(key: Crossterm) -> Input {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    // Crossterm's Unix parser represents the legacy 0x1d byte as Ctrl+5.
    if key.code == KeyCode::Char('\u{1d}')
        || (ctrl
            && (key.code == KeyCode::Char(']') || cfg!(unix) && key.code == KeyCode::Char('5')))
    {
        return match key.kind {
            KeyEventKind::Release => Input::Ignore,
            _ => Input::Detach,
        };
    }
    key_event(key).map_or(Input::Ignore, Input::Key)
}

pub fn key_event(key: Crossterm) -> Option<KeyEvent> {
    let mut modifiers = Mods::NONE;
    modifiers.set(Mods::SHIFT, key.modifiers.contains(KeyModifiers::SHIFT));
    modifiers.set(Mods::CONTROL, key.modifiers.contains(KeyModifiers::CONTROL));
    modifiers.set(Mods::ALT, key.modifiers.contains(KeyModifiers::ALT));
    modifiers.set(Mods::SUPER, key.modifiers.contains(KeyModifiers::SUPER));
    let action = match key.kind {
        KeyEventKind::Press => KeyAction::Press,
        KeyEventKind::Repeat => KeyAction::Repeat,
        KeyEventKind::Release => KeyAction::Release,
    };
    let physical = match key.code {
        KeyCode::Char(c) => return character(c, modifiers, action),
        KeyCode::Enter => P::Enter,
        KeyCode::Tab => P::Tab,
        KeyCode::BackTab => {
            modifiers.set(Mods::SHIFT, true);
            P::Tab
        }
        KeyCode::Backspace => P::Backspace,
        KeyCode::Esc => P::Escape,
        KeyCode::Up => P::ArrowUp,
        KeyCode::Down => P::ArrowDown,
        KeyCode::Left => P::ArrowLeft,
        KeyCode::Right => P::ArrowRight,
        KeyCode::Home => P::Home,
        KeyCode::End => P::End,
        KeyCode::PageUp => P::PageUp,
        KeyCode::PageDown => P::PageDown,
        KeyCode::Insert => P::Insert,
        KeyCode::Delete => P::Delete,
        KeyCode::Menu => P::ContextMenu,
        KeyCode::F(n @ 1..=25) => P::try_from(P::F1.code() + u32::from(n) - 1).ok()?,
        _ => return None,
    };
    Some(KeyEvent {
        action,
        ..KeyEvent::press(physical, modifiers)
    })
}

/// Crossterm reports the character the layout produced, with Shift applied
/// and Ctrl not. Its US key position, if it has one, is the physical key.
fn character(c: char, modifiers: Mods, action: KeyAction) -> Option<KeyEvent> {
    if c.is_control() {
        return None;
    }
    let base = c.to_lowercase().next().unwrap_or(c);
    let physical = match base {
        'a'..='z' => P::try_from(P::KeyA.code() + u32::from(base) - u32::from('a')).ok()?,
        '0'..='9' => P::try_from(P::Digit0.code() + u32::from(base) - u32::from('0')).ok()?,
        ' ' => P::Space,
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
    };
    let shifted = base != c;
    Some(KeyEvent {
        physical,
        action,
        modifiers,
        consumed_modifiers: if shifted { Mods::SHIFT } else { Mods::NONE },
        text: match action {
            KeyAction::Release => String::new(),
            _ => c.to_string(),
        },
        unshifted_codepoint: Some(base),
        composing: false,
    })
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
