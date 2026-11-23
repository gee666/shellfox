//! Key input for Ghostty's encoder. Clients describe the key; the terminal
//! that owns the modes (DECCKM, keypad, kitty flags, modifyOtherKeys) encodes it.
use crate::{sys, Error, Result, Terminal};
pub use sys::{validate_text, InvalidKey, KeyAction, KeyModifiers, PhysicalKey};

/// Owned key event. `text` is the layout text without Ctrl applied, never
/// control characters; `unshifted_codepoint` is the key's base character.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyEvent {
    pub physical: PhysicalKey,
    pub action: KeyAction,
    pub modifiers: KeyModifiers,
    pub consumed_modifiers: KeyModifiers,
    pub text: String,
    #[serde(with = "scalar")]
    pub unshifted_codepoint: Option<char>,
    pub composing: bool,
}

/// The unshifted character travels as its number, like the C ABI.
mod scalar {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &Option<char>, s: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(c) => s.serialize_some(&u32::from(*c)),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<char>, D::Error> {
        Option::<u32>::deserialize(d)?
            .map(|n| {
                char::from_u32(n)
                    .filter(|c| *c != char::MIN)
                    .ok_or_else(|| D::Error::custom("invalid scalar"))
            })
            .transpose()
    }
}
impl KeyEvent {
    pub fn press(physical: PhysicalKey, modifiers: KeyModifiers) -> Self {
        Self {
            physical,
            action: KeyAction::Press,
            modifiers,
            consumed_modifiers: KeyModifiers::NONE,
            text: String::new(),
            unshifted_codepoint: None,
            composing: false,
        }
    }
    /// Committed text with no key behind it (IME, voice, emoji, autocorrect).
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::press(PhysicalKey::Unidentified, KeyModifiers::NONE)
        }
    }
    /// The matching release: same key and modifiers, no text.
    pub fn release(&self) -> Self {
        Self {
            action: KeyAction::Release,
            text: String::new(),
            ..self.clone()
        }
    }
    pub fn validate(&self) -> std::result::Result<(), InvalidKey> {
        self.borrowed().validate()
    }
    fn borrowed(&self) -> sys::KeyEvent<'_> {
        sys::KeyEvent {
            physical: self.physical,
            action: self.action,
            modifiers: self.modifiers,
            consumed_modifiers: self.consumed_modifiers,
            text: &self.text,
            unshifted_codepoint: self.unshifted_codepoint,
            composing: self.composing,
        }
    }
}
impl std::fmt::Debug for KeyEvent {
    // Typed text can be a password; log its size only.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyEvent")
            .field("physical", &self.physical)
            .field("action", &self.action)
            .field("modifiers", &self.modifiers)
            .field("consumed_modifiers", &self.consumed_modifiers)
            .field("text_bytes", &self.text.len())
            .field("composing", &self.composing)
            .finish()
    }
}

impl Terminal {
    pub fn encode_key_event(&mut self, event: &KeyEvent) -> Result<Vec<u8>> {
        self.engine.encode_key(&event.borrowed()).map_err(Error)
    }
}
