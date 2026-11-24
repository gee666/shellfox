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
        let bytes = self.engine.encode_key(&event.borrowed()).map_err(Error)?;
        if event.physical == PhysicalKey::F3 && self.engine.kitty_key_flags().map_err(Error)? == 0 {
            return Ok(xterm_modified_f3(bytes));
        }
        Ok(bytes)
    }
}

/// Legacy modified F3 as xterm sends it, `CSI 1;<m>R`, which is what the
/// xterm-256color terminfo declares (kf15, kf27, ...). Ghostty sends
/// `CSI 13;<m>~` to keep it apart from a cursor position report, but
/// curses applications like c3270 then never see Shift+F3 (PF15).
fn xterm_modified_f3(bytes: Vec<u8>) -> Vec<u8> {
    match bytes
        .strip_prefix(b"\x1b[13;")
        .and_then(|rest| rest.strip_suffix(b"~"))
    {
        Some(m) if !m.is_empty() && m.iter().all(u8::is_ascii_digit) => {
            [b"\x1b[1;".as_slice(), m, b"R"].concat()
        }
        _ => bytes,
    }
}

/// The US-layout key that types `c`, its unshifted character, and whether
/// Shift is needed. Frontends that report characters rather than scan codes
/// (GPUI, crossterm) use this so press and release name the same key.
pub fn us_key(c: char) -> Option<(PhysicalKey, char, bool)> {
    const SHIFTED: [(char, char); 21] = [
        ('!', '1'),
        ('@', '2'),
        ('#', '3'),
        ('$', '4'),
        ('%', '5'),
        ('^', '6'),
        ('&', '7'),
        ('*', '8'),
        ('(', '9'),
        (')', '0'),
        ('~', '`'),
        ('_', '-'),
        ('+', '='),
        ('{', '['),
        ('}', ']'),
        ('|', '\\'),
        (':', ';'),
        ('"', '\''),
        ('<', ','),
        ('>', '.'),
        ('?', '/'),
    ];
    let (base, shifted) = match c {
        'A'..='Z' => (c.to_ascii_lowercase(), true),
        c => SHIFTED
            .iter()
            .find(|(s, _)| *s == c)
            .map_or((c, false), |&(_, b)| (b, true)),
    };
    let offset = |from: char| u32::from(base) - u32::from(from);
    let physical = match base {
        'a'..='z' => PhysicalKey::try_from(PhysicalKey::KeyA.code() + offset('a')).ok()?,
        '0'..='9' => PhysicalKey::try_from(PhysicalKey::Digit0.code() + offset('0')).ok()?,
        ' ' => PhysicalKey::Space,
        '`' => PhysicalKey::Backquote,
        '-' => PhysicalKey::Minus,
        '=' => PhysicalKey::Equal,
        '[' => PhysicalKey::BracketLeft,
        ']' => PhysicalKey::BracketRight,
        '\\' => PhysicalKey::Backslash,
        ';' => PhysicalKey::Semicolon,
        '\'' => PhysicalKey::Quote,
        ',' => PhysicalKey::Comma,
        '.' => PhysicalKey::Period,
        '/' => PhysicalKey::Slash,
        _ => return None,
    };
    Some((physical, base, shifted))
}

/// Keys whose press reached a terminal. Repeats and releases of keys whose
/// press never did (an app shortcut, the key that launched a CLI) are dropped.
#[derive(Default)]
pub struct HeldKeys(Vec<KeyEvent>);
impl HeldKeys {
    /// Holds at most this many keys; more presses still go through.
    pub const MAX: usize = 64;
    /// Whether `event` may go to the terminal. Records presses and releases.
    /// Committed text has no key identity and always goes.
    pub fn admit(&mut self, event: &KeyEvent) -> bool {
        let Some(key) = event.identity() else {
            return true;
        };
        let held = self.0.iter().any(|h| h.identity() == Some(key));
        if !held && event.action != KeyAction::Press {
            return false;
        }
        self.0.retain(|h| h.identity() != Some(key));
        if event.action != KeyAction::Release && self.0.len() < Self::MAX {
            self.0.push(event.clone());
        }
        true
    }
    /// The release for a held key, found by what a key-up names.
    pub fn release(&self, physical: PhysicalKey, unshifted: Option<char>) -> Option<KeyEvent> {
        let key = (physical, unshifted);
        self.0
            .iter()
            .find(|h| h.identity() == Some(key))
            .map(KeyEvent::release)
    }
    /// Releases for everything held, e.g. on focus loss. Clears the set.
    pub fn release_all(&mut self) -> Vec<KeyEvent> {
        self.0.drain(..).map(|h| h.release()).collect()
    }
}
impl KeyEvent {
    /// The physical key and base character, or none for committed text.
    pub fn identity(&self) -> Option<(PhysicalKey, Option<char>)> {
        (self.physical != PhysicalKey::Unidentified || self.unshifted_codepoint.is_some())
            .then_some((self.physical, self.unshifted_codepoint))
    }
}
