//! Typed key input for the pinned public Ghostty key encoder.
mod modifiers;
mod physical;
#[cfg(test)]
mod tests;
use crate::LIMIT;
pub use modifiers::KeyModifiers;
pub use physical::PhysicalKey;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyAction {
    Release = 0,
    Press = 1,
    Repeat = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidKey {
    PhysicalKey(u32),
    ModifierBits(u32),
    Scalar(u32),
    TextCodepoint(char),
    TextLimit(usize),
}
impl InvalidKey {
    pub(crate) const fn native_code(self) -> i32 {
        if matches!(self, Self::TextLimit(_)) {
            -6
        } else {
            -2
        }
    }
}
impl std::fmt::Display for InvalidKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::PhysicalKey(v) => write!(f, "unknown physical key {v}"),
            Self::ModifierBits(v) => write!(f, "unknown modifier bits in {v:#x}"),
            Self::Scalar(v) => write!(f, "unrepresentable unshifted scalar U+{v:04X}"),
            Self::TextCodepoint(c) => {
                write!(f, "disallowed key text codepoint U+{:04X}", u32::from(c))
            }
            Self::TextLimit(n) => write!(f, "key text exceeds allocation bound: {n} bytes"),
        }
    }
}
impl std::error::Error for InvalidKey {}

/// One key event as Ghostty's encoder sees it. Physical identity and layout
/// text are independent; nothing is guessed from the other.
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent<'a> {
    pub physical: PhysicalKey,
    pub action: KeyAction,
    pub modifiers: KeyModifiers,
    /// Modifiers the layout used to produce `text`, e.g. Shift for "A".
    pub consumed_modifiers: KeyModifiers,
    /// Layout text without Ctrl applied. Never control characters.
    pub text: &'a str,
    pub unshifted_codepoint: Option<char>,
    /// A dead key or IME composition is in progress.
    pub composing: bool,
}
impl KeyEvent<'_> {
    pub fn validate(&self) -> Result<(), InvalidKey> {
        if self.unshifted_codepoint == Some('\0') {
            return Err(InvalidKey::Scalar(0));
        }
        validate_text(self.text)
    }
}

/// Ghostty's public input guide: C0, DEL and the macOS function-key private
/// range are not key text. Other C1/format characters are valid text.
pub fn validate_text(text: &str) -> Result<(), InvalidKey> {
    if text.len() > LIMIT {
        return Err(InvalidKey::TextLimit(text.len()));
    }
    match text
        .chars()
        .find(|&c| c <= '\u{1f}' || c == '\u{7f}' || ('\u{f700}'..='\u{f8ff}').contains(&c))
    {
        Some(c) => Err(InvalidKey::TextCodepoint(c)),
        None => Ok(()),
    }
}
