use crate::{Error, Result};
use shellfox_ghostty_vt_sys as sys;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KeyAction {
    #[default]
    Press,
    Repeat,
    Release,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}
impl Modifiers {
    pub(crate) fn bits(self) -> u32 {
        u32::from(self.shift)
            | u32::from(self.control) << 1
            | u32::from(self.alt) << 2
            | u32::from(self.super_key) << 3
    }
}
/// Physical key identity, independent of committed text. Character identifies
/// an ASCII physical position, not the text produced by the current layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Key {
    #[default]
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
    F(u8),
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEvent {
    pub key: Key,
    pub action: KeyAction,
    pub modifiers: Modifiers,
    pub consumed_modifiers: Modifiers,
    pub text: Option<String>,
    pub unshifted_codepoint: u32,
    pub composing: bool,
}
impl Key {
    // Public GhosttyKey discriminants in vt-1.0.1 key/event.h. These values
    // belong to the stable C ABI, not Ghostty's internal key representation.
    fn code(self) -> Result<u32> {
        Ok(match self {
            Self::Unidentified => 0,
            Self::Enter => 58,
            Self::Tab => 64,
            Self::Backspace => 53,
            Self::Escape => 120,
            Self::ArrowUp => 78,
            Self::ArrowDown => 75,
            Self::ArrowLeft => 76,
            Self::ArrowRight => 77,
            Self::Home => 71,
            Self::End => 69,
            Self::PageUp => 74,
            Self::PageDown => 73,
            Self::Insert => 72,
            Self::Delete => 68,
            Self::F(n) if (1..=25).contains(&n) => 120 + u32::from(n),
            Self::F(_) => return Err(Error(-2)),
            Self::Character(c) => match c.to_ascii_lowercase() {
                'a'..='z' => 20 + u32::from(c.to_ascii_lowercase()) - u32::from('a'),
                '0'..='9' => 6 + u32::from(c) - u32::from('0'),
                '`' => 1,
                '\\' => 2,
                '[' => 3,
                ']' => 4,
                ',' => 5,
                '=' => 16,
                '-' => 46,
                '.' => 47,
                '\'' => 48,
                ';' => 49,
                '/' => 50,
                ' ' => 63,
                _ => 0,
            },
        })
    }
}
impl KeyEvent {
    pub(crate) fn native(&self) -> Result<sys::KeyEvent<'_>> {
        let text = self.text.as_deref().unwrap_or("");
        if text
            .chars()
            .any(|c| c.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&c))
        {
            return Err(Error(-2));
        }
        let unshifted_codepoint = match self.unshifted_codepoint {
            0 => None,
            value => Some(char::from_u32(value).ok_or(Error(-2))?),
        };
        let mods = |m: Modifiers| sys::KeyModifiers::try_from_bits(m.bits()).map_err(|_| Error(-2));
        Ok(sys::KeyEvent {
            physical: sys::PhysicalKey::try_from(self.key.code()?).map_err(|_| Error(-2))?,
            action: match self.action {
                KeyAction::Press => sys::KeyAction::Press,
                KeyAction::Release => sys::KeyAction::Release,
                KeyAction::Repeat => sys::KeyAction::Repeat,
            },
            modifiers: mods(self.modifiers)?,
            consumed_modifiers: mods(self.consumed_modifiers)?,
            unshifted_codepoint,
            composing: self.composing,
            text,
        })
    }
}
