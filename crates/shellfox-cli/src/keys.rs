//! Crossterm events as Ghostty key events. A Ghostty terminal that knows the
//! application's modes encodes them; nothing here produces terminal bytes.
use crossterm::event::{Event, KeyCode, KeyEvent as Crossterm, KeyEventKind, KeyModifiers};
use shellfox_ghostty_vt::key::{us_key, HeldKeys, KeyAction, KeyEvent, KeyModifiers as Mods, PhysicalKey as P};

#[derive(Debug, PartialEq)]
pub enum Input {
    Detach,
    Key(KeyEvent),
    Paste(String),
    Ignore,
}

/// Crossterm events for one terminal. Releases and repeats whose press this
/// terminal never got (the Enter that started the command) are dropped.
#[derive(Default)]
pub struct Keys(HeldKeys);
impl Keys {
    // Never classify response-looking user text as an automatic terminal reply.
    // Queries are suppressed on the output path, not guessed from ambiguous input.
    pub fn input(&mut self, event: Event) -> Input {
        match event {
            Event::Key(key) if detach(&key) => match key.kind {
                KeyEventKind::Release => Input::Ignore,
                _ => Input::Detach,
            },
            Event::Key(key) => self.key(key).map_or(Input::Ignore, Input::Key),
            Event::Paste(text) => Input::Paste(text),
            _ => Input::Ignore,
        }
    }
    /// One key for the terminal, with no local detach key (ssh).
    pub fn key(&mut self, key: Crossterm) -> Option<KeyEvent> {
        let event = key_event(key)?;
        if self.0.admit(&event) {
            return Some(event);
        }
        match key.code {
            // Windows delivers an Alt code (Alt+numpad) only as a release
            // carrying its character.
            KeyCode::Char(c) if cfg!(windows) && event.action == KeyAction::Release && !c.is_control() => {
                Some(KeyEvent::text(c))
            }
            _ => None,
        }
    }
}

/// Ctrl+] detaches. Crossterm's Unix parser reports the 0x1d byte as Ctrl+5.
/// Ctrl+Alt+] is AltGr+] on Windows layouts, which types ].
fn detach(key: &Crossterm) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    key.code == KeyCode::Char('\u{1d}')
        || ctrl && (key.code == KeyCode::Char(']') || cfg!(unix) && key.code == KeyCode::Char('5'))
}

fn key_event(key: Crossterm) -> Option<KeyEvent> {
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
    // Windows reports AltGr as Ctrl+Alt with the character it typed.
    let altgr = cfg!(windows) && modifiers.contains(Mods::CONTROL | Mods::ALT) && !c.is_ascii_alphanumeric();
    if altgr {
        return Some(KeyEvent { action, ..KeyEvent::text(c) });
    }
    let (physical, base, shifted) = us_key(c).unwrap_or((P::Unidentified, c.to_lowercase().next().unwrap_or(c), false));
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
