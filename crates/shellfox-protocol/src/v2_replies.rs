//! Private shared v2 envelope codec. Profiles retain separate binding/error DTOs.
use crate::{
    observer_v2::Version,
    validation::{ensure, present_nullable},
    Validate, ValidationError,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug)]
pub enum Reply<T, E> {
    Success(T),
    Failure(E),
}
#[derive(Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    bound(deserialize = "T: Deserialize<'de>, E: Deserialize<'de>")
)]
struct Raw<T, E> {
    version: Version,
    ok: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    value: Option<Option<T>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    error: Option<Option<E>>,
}
impl<T, E> Raw<T, E> {
    fn outcome<D: serde::de::Error>(self) -> Result<Reply<T, E>, D> {
        match (self.ok, self.value, self.error) {
            (true, Some(Some(value)), None) => Ok(Reply::Success(value)),
            (false, None, Some(Some(error))) => Ok(Reply::Failure(error)),
            _ => Err(D::custom("Invalid v2 result branch.")),
        }
    }
}
impl<T: Serialize, E: Serialize> Serialize for Reply<T, E> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Success(v) => Raw::<_, &E> {
                version: Version,
                ok: true,
                value: Some(Some(v)),
                error: None,
            }
            .serialize(s),
            Self::Failure(e) => Raw::<&T, _> {
                version: Version,
                ok: false,
                value: None,
                error: Some(Some(e)),
            }
            .serialize(s),
        }
    }
}
impl<'de, T: Deserialize<'de>, E: Deserialize<'de>> Deserialize<'de> for Reply<T, E> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Raw::<T, E>::deserialize(d)?.outcome()
    }
}
impl<T: Validate, E: Validate> Validate for Reply<T, E> {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Success(v) => v.validate(),
            Self::Failure(e) => e.validate(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct BoundReply<T, B, E> {
    pub binding: B,
    pub outcome: Reply<T, E>,
}
#[derive(Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    bound(deserialize = "T: Deserialize<'de>, B: Deserialize<'de>, E: Deserialize<'de>")
)]
struct BoundRaw<T, B, E> {
    version: Version,
    binding: B,
    ok: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    value: Option<Option<T>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    error: Option<Option<E>>,
}
impl<T: Serialize, B: Serialize, E: Serialize> Serialize for BoundReply<T, B, E> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.outcome {
            Reply::Success(v) => BoundRaw::<_, _, &E> {
                version: Version,
                binding: &self.binding,
                ok: true,
                value: Some(Some(v)),
                error: None,
            }
            .serialize(s),
            Reply::Failure(e) => BoundRaw::<&T, _, _> {
                version: Version,
                binding: &self.binding,
                ok: false,
                value: None,
                error: Some(Some(e)),
            }
            .serialize(s),
        }
    }
}
impl<'de, T: Deserialize<'de>, B: Deserialize<'de>, E: Deserialize<'de>> Deserialize<'de>
    for BoundReply<T, B, E>
{
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = BoundRaw::<T, B, E>::deserialize(d)?;
        let outcome = Raw {
            version: v.version,
            ok: v.ok,
            value: v.value,
            error: v.error,
        }
        .outcome()?;
        Ok(Self {
            binding: v.binding,
            outcome,
        })
    }
}
impl<T: Validate, B: Validate, E: Validate> Validate for BoundReply<T, B, E> {
    fn validate(&self) -> Result<(), ValidationError> {
        self.binding.validate()?;
        self.outcome.validate()
    }
}
impl<T, B: PartialEq, E> BoundReply<T, B, E> {
    pub fn check_binding(&self, expected: &B) -> Result<(), ValidationError> {
        ensure(self.binding == *expected)
    }
}
