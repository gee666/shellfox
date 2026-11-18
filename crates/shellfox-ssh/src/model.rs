use crate::{endpoint::text, normalize_endpoint, Error, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use uuid::Uuid;
use zeroize::Zeroizing;

fn required_optional<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d)
}

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<String>);
impl Secret {
    pub fn new(value: String) -> Result<Self> {
        let value = Zeroizing::new(value);
        if value.encode_utf16().count() > 65536 {
            return Err(Error::Limit);
        }
        Ok(Self(value))
    }
    /// Host adapter only. Never send this value to remote clients or logs.
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}
impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.expose())
    }
}
impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// Missing JSON password preserves the previous value; null clears; "" is a password.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub enum PasswordUpdate {
    #[default]
    Keep,
    Clear,
    Set(Secret),
}
impl PasswordUpdate {
    fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}
impl Serialize for PasswordUpdate {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Set(secret) => secret.serialize(s),
            _ => s.serialize_none(),
        }
    }
}
impl<'de> Deserialize<'de> for PasswordUpdate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(Option::<Secret>::deserialize(d)?.map_or(Self::Clear, Self::Set))
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileInput {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default, skip_serializing_if = "PasswordUpdate::is_keep")]
    pub password: PasswordUpdate,
    #[serde(deserialize_with = "required_optional")]
    pub key_file: Option<String>,
    #[serde(deserialize_with = "required_optional")]
    pub remote_cwd: Option<String>,
}
impl ProfileInput {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 512 * 1024 {
            return Err(Error::Limit);
        }
        serde_json::from_slice::<Self>(bytes)
            .map_err(|_| Error::Validation)?
            .validate()
    }
    pub fn validate(mut self) -> Result<Self> {
        text(&self.name, 200)?;
        self.name = self
            .name
            .trim_matches(crate::endpoint::js_whitespace)
            .to_owned();
        if self.name.is_empty() || self.port == 0 {
            return Err(Error::Validation);
        }
        (self.host, self.user) = normalize_endpoint(&self.host, &self.user)?;
        for value in [&self.key_file, &self.remote_cwd].into_iter().flatten() {
            text(value, 32767)?;
        }
        Ok(self)
    }
}

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_family: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub try_agent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_forward: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_proxy: Option<UnsupportedProxy>,
}
impl fmt::Debug for ConnectionOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConnectionOptions([HOST-ONLY])")
    }
}
impl ConnectionOptions {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.address_family.is_some_and(|v| ![0, 4, 6].contains(&v)) {
            return Err(Error::Validation);
        }
        if let Some(proxy) = &self.unsupported_proxy {
            text(&proxy.method, 100)?;
            if let Some(host) = &proxy.host {
                text(host, 1000)?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnsupportedProxy {
    pub method: String,
    pub host: Option<String>,
    pub port: Option<u16>,
}
impl fmt::Debug for UnsupportedProxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UnsupportedProxy([HOST-ONLY])")
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSource {
    Manual,
    Putty,
}

/// Host-only, serializable solely for the private store. Debug redacts passwords.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredProfile {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(deserialize_with = "required_optional")]
    pub password: Option<Secret>,
    #[serde(deserialize_with = "required_optional")]
    pub key_file: Option<String>,
    #[serde(deserialize_with = "required_optional")]
    pub remote_cwd: Option<String>,
    pub source: ProfileSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_options: Option<ConnectionOptions>,
}
impl StoredProfile {
    pub(crate) fn normalize(mut self) -> Result<Self> {
        let input = ProfileInput {
            id: self.id,
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            password: PasswordUpdate::Keep,
            key_file: self.key_file.clone(),
            remote_cwd: self.remote_cwd.clone(),
        }
        .validate()?;
        self.name = input.name;
        self.host = input.host;
        self.user = input.user;
        if let Some(options) = &self.connection_options {
            options.validate()?;
        }
        Ok(self)
    }
    pub fn remote_dto(&self) -> RemoteProfileDto {
        RemoteProfileDto {
            id: self.id,
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            source: self.source,
            has_password: self.password.is_some(),
        }
    }
    pub fn dto(&self) -> ProfileDto {
        ProfileDto {
            id: self.id,
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            key_file: self.key_file.clone(),
            remote_cwd: self.remote_cwd.clone(),
            source: self.source,
            has_password: self.password.is_some(),
        }
    }
}

/// Sanitized DTO matching the desktop settings contract. No secret or private options.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileDto {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_file: Option<String>,
    pub remote_cwd: Option<String>,
    pub source: ProfileSource,
    pub has_password: bool,
}
/// Remote-only clients get neither host key-file paths nor credential-bearing options.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteProfileDto {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub source: ProfileSource,
    pub has_password: bool,
}
#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSession {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_file: Option<String>,
    pub connection_options: Option<ConnectionOptions>,
}
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub profiles: Vec<ProfileDto>,
    pub added: usize,
    pub updated: usize,
    pub found: usize,
}
