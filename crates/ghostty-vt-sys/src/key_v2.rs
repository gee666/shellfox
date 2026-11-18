//! Additive typed input. Encoding always uses the pinned public Ghostty encoder.
mod modifiers;
mod physical;
#[cfg(test)]
mod tests;
use crate::{Key, LIMIT};
pub use modifiers::ModifiersV2;
pub use physical::PhysicalKey;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyActionV2 {
    Release = 0,
    Press = 1,
    Repeat = 2,
}
impl TryFrom<u32> for KeyActionV2 {
    type Error = InvalidKeyV2;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Release),
            1 => Ok(Self::Press),
            2 => Ok(Self::Repeat),
            _ => Err(InvalidKeyV2::Action(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidKeyV2 {
    PhysicalKey(u32),
    Action(u32),
    ModifierBits(u32),
    Scalar(u32),
    TextCodepoint(char),
    TextLimit(usize),
}
impl InvalidKeyV2 {
    pub(crate) const fn native_code(self) -> i32 {
        if matches!(self, Self::TextLimit(_)) {
            -6
        } else {
            -2
        }
    }
}
impl std::fmt::Display for InvalidKeyV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::PhysicalKey(v) => write!(f, "unknown physical key {v}"),
            Self::Action(v) => write!(f, "unknown key action {v}"),
            Self::ModifierBits(v) => write!(f, "unknown modifier bits in {v:#x}"),
            Self::Scalar(v) => write!(f, "unrepresentable unshifted scalar U+{v:04X}"),
            Self::TextCodepoint(c) => {
                write!(f, "disallowed key text codepoint U+{:04X}", u32::from(c))
            }
            Self::TextLimit(n) => write!(f, "key text exceeds allocation bound: {n} bytes"),
        }
    }
}
impl std::error::Error for InvalidKeyV2 {}

/// Physical identity and layout text are independent. All fields are explicit;
/// no missing physical identity or modifier state is guessed.
#[derive(Debug, Clone, Copy)]
pub struct KeyV2<'a> {
    pub physical: PhysicalKey,
    pub action: KeyActionV2,
    pub modifiers: ModifiersV2,
    /// A mask, not a second physical keyboard state. Never pre-subtract it.
    pub consumed_modifiers: ModifiersV2,
    /// Unmodified layout UTF-8, borrowed only for the synchronous encode call.
    pub text: &'a str,
    /// None is the public zero sentinel. Some NUL is rejected, not normalized.
    pub unshifted_codepoint: Option<char>,
    /// Preedit remains a UI concern; the public encoder decides suppression.
    pub composing: bool,
}
impl KeyV2<'_> {
    pub fn validate(&self) -> Result<(), InvalidKeyV2> {
        if self.unshifted_codepoint == Some('\0') {
            return Err(InvalidKeyV2::Scalar(0));
        }
        validate_raw(&self.raw())
    }
    pub(crate) fn raw(&self) -> Key<'_> {
        Key {
            key: self.physical.code(),
            action: self.action as u32,
            mods: u32::from(self.modifiers.bits()),
            consumed_mods: u32::from(self.consumed_modifiers.bits()),
            unshifted: self.unshifted_codepoint.map_or(0, u32::from),
            composing: self.composing,
            text: self.text,
        }
    }
}

pub(crate) fn validate_raw(key: &Key<'_>) -> Result<(), InvalidKeyV2> {
    PhysicalKey::try_from(key.key)?;
    KeyActionV2::try_from(key.action)?;
    ModifiersV2::try_from_bits(key.mods)?;
    ModifiersV2::try_from_bits(key.consumed_mods)?;
    if key.unshifted != 0 && char::from_u32(key.unshifted).is_none() {
        return Err(InvalidKeyV2::Scalar(key.unshifted));
    }
    if key.text.len() > LIMIT {
        return Err(InvalidKeyV2::TextLimit(key.text.len()));
    }
    for c in key.text.chars() {
        if c <= '\u{1f}' || c == '\u{7f}' || ('\u{f700}'..='\u{f8ff}').contains(&c) {
            return Err(InvalidKeyV2::TextCodepoint(c));
        }
    }
    Ok(())
}
