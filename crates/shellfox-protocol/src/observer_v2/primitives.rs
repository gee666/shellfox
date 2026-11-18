use crate::{validation::ensure, Validate, ValidationError};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Version;
impl TryFrom<u8> for Version {
    type Error = ValidationError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        ensure(value == 2)?;
        Ok(Self)
    }
}
impl From<Version> for u8 {
    fn from(_: Version) -> Self {
        2
    }
}

/// An identifier, not proof of a peer, process, input grant or cleanup authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(uuid::Uuid);
impl Id {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
    pub fn uuid(self) -> uuid::Uuid {
        self.0
    }
}
impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}
impl FromStr for Id {
    type Err = ValidationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        ensure(crate::ids::is_wire_id(value))?;
        let uuid = uuid::Uuid::parse_str(value).map_err(|_| ValidationError)?;
        ensure(!uuid.is_nil())?;
        Ok(Self(uuid))
    }
}
impl TryFrom<String> for Id {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}
impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0.to_string()
    }
}
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

macro_rules! literal {
    ($name:ident,$variant:ident,$wire:literal) => {
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
        pub enum $name {
            #[default]
            #[serde(rename=$wire)]
            $variant,
        }
    };
}
literal!(Profile, Observer, "observer-v2");
literal!(CatalogMode, Complete, "complete");
literal!(DeliveryFormat, VtBytes, "vt-bytes-base64-v1");
literal!(
    RecoveryFormat,
    FormattedActiveBuffer,
    "formatted-active-buffer-v1"
);
literal!(Encoding, Base64, "base64");
literal!(ServiceName, Shellfox, "shellfox");

pub(crate) fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<T, D::Error> {
    T::deserialize(d)
}
pub(crate) fn base64_size(data: &str, max: usize, empty: bool) -> Result<usize, ValidationError> {
    ensure((empty || !data.is_empty()) && data.len() <= max.div_ceil(3) * 4)?;
    let size = crate::chunks::base64_size(data)?;
    ensure(size <= max)?;
    Ok(size)
}
pub fn decode_bytes(data: &str, max: usize) -> Result<Vec<u8>, ValidationError> {
    ensure(max <= super::MAX_RECOVERY_BYTES)?;
    let size = base64_size(data, max, true)?;
    let mut out = Vec::with_capacity(size);
    for part in data.as_bytes().chunks_exact(4) {
        let value = |b: u8| -> u32 {
            match b {
                b'A'..=b'Z' => u32::from(b - b'A'),
                b'a'..=b'z' => u32::from(b - b'a' + 26),
                b'0'..=b'9' => u32::from(b - b'0' + 52),
                b'+' => 62,
                b'/' => 63,
                _ => 0,
            }
        };
        let n = (value(part[0]) << 18)
            | (value(part[1]) << 12)
            | (value(part[2]) << 6)
            | value(part[3]);
        out.push((n >> 16) as u8);
        if part[2] != b'=' {
            out.push((n >> 8) as u8);
        }
        if part[3] != b'=' {
            out.push(n as u8);
        }
    }
    Ok(out)
}
impl Validate for Version {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
