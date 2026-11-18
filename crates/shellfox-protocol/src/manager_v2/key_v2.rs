//! Matching full KeyV2 wire model; native capture/owner conversion adoption is coordinated separately.
use super::{PhysicalKey, MAX_INPUT_PAYLOAD_BYTES};
use crate::{validation::ensure, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyActionV2 {
    Release = 0,
    Press = 1,
    Repeat = 2,
}
impl KeyActionV2 {
    pub const fn code(self) -> u32 {
        self as u32
    }
}
/// All defined patterns survive, including side-only release/consumed masks.
/// Unknown bits reject BEFORE u16 narrowing. No side/base or consumed-subset inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct ModifiersV2(u16);
impl ModifiersV2 {
    pub const NONE: Self = Self(0);
    pub fn try_from_bits(bits: u32) -> Result<Self, ValidationError> {
        ensure(bits & !0x03ff == 0)?;
        u16::try_from(bits).map(Self).map_err(|_| ValidationError)
    }
    pub const fn bits(self) -> u16 {
        self.0
    }
}
impl TryFrom<u32> for ModifiersV2 {
    type Error = ValidationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from_bits(value)
    }
}
impl From<ModifiersV2> for u32 {
    fn from(value: ModifiersV2) -> Self {
        u32::from(value.bits())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct UnshiftedScalar(char);
impl UnshiftedScalar {
    pub fn new(value: char) -> Result<Self, ValidationError> {
        ensure(value != '\0')?;
        Ok(Self(value))
    }
    pub const fn get(self) -> char {
        self.0
    }
}
impl TryFrom<u32> for UnshiftedScalar {
    type Error = ValidationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(char::from_u32(value).ok_or(ValidationError)?)
    }
}
impl From<UnshiftedScalar> for u32 {
    fn from(value: UnshiftedScalar) -> Self {
        u32::from(value.0)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyEventV2 {
    pub physical: PhysicalKey,
    pub action: KeyActionV2,
    pub modifiers: ModifiersV2,
    pub consumed_modifiers: ModifiersV2,
    pub text: String,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub unshifted_codepoint: Option<UnshiftedScalar>,
    pub composing: bool,
}
impl KeyEventV2 {
    pub fn unshifted_char(&self) -> Option<char> {
        self.unshifted_codepoint.map(UnshiftedScalar::get)
    }
    pub fn text_utf8(&self) -> &str {
        &self.text
    }
}
impl Validate for KeyEventV2 {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.text.len() <= MAX_INPUT_PAYLOAD_BYTES)?;
        ensure(
            !self
                .text
                .chars()
                .any(|c| c <= '\u{1f}' || c == '\u{7f}' || ('\u{f700}'..='\u{f8ff}').contains(&c)),
        )
    }
}
