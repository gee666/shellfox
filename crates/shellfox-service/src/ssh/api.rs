//! Store operations for in-process and authenticated attached Settings.
use anyhow::Result;
use shellfox_ssh::{ImportResult, ProfileDto, Secret};
use std::path::PathBuf;
use uuid::Uuid;

/// Deliberately not Debug or Serialize. Empty Set is distinct from Keep/Clear.
#[derive(Clone)]
pub enum Password {
    Keep,
    Clear,
    Set(Secret),
}
impl Password {
    pub fn set(value: String) -> Result<Self> {
        Ok(Self::Set(Secret::new(value)?))
    }
}
#[derive(Clone)]
pub struct Input {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Password,
    pub key_file: Option<String>,
    pub remote_cwd: Option<String>,
}
#[derive(Clone)]
pub enum Request {
    List,
    Save(Input),
    Delete { id: Uuid },
    Import,
    ValidateKey { selected: SelectedKey },
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Response {
    Profiles(Vec<ProfileDto>),
    Imported(ImportResult),
    SelectedKey(SelectedKeyInfo),
}
/// A validated local key path, never accepted from the remote protocol.
#[derive(Clone)]
pub struct SelectedKey(PathBuf);
impl SelectedKey {
    pub fn new(path: PathBuf) -> Result<Self> {
        anyhow::ensure!(
            path.is_absolute()
                && path
                    .to_str()
                    .is_some_and(|s| s.len() <= 32767 && !s.chars().any(char::is_control)),
            "Invalid selected SSH key path."
        );
        Ok(Self(path))
    }
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl std::fmt::Debug for SelectedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SelectedKey([HOST-ONLY])")
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedKeyInfo {
    pub path: String,
    pub metadata: shellfox_ssh::key_metadata::KeyMetadata,
    pub cryptographically_validated: bool,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::List => "List",
            Self::Save(_) => "Save([HOST-ONLY])",
            Self::Delete { .. } => "Delete",
            Self::Import => "Import",
            Self::ValidateKey { .. } => "ValidateKey([HOST-ONLY])",
        };
        write!(f, "SshRequest::{name}")
    }
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Save(input) => {
                anyhow::ensure!(!input.id.is_nil(), "SSH profile ID is required.");
                input.clone().into_profile().validate()?;
            }
            Self::Delete { id } => anyhow::ensure!(!id.is_nil(), "SSH profile ID is required."),
            _ => {}
        }
        Ok(())
    }
}
impl Input {
    pub(crate) fn into_profile(self) -> shellfox_ssh::ProfileInput {
        shellfox_ssh::ProfileInput {
            id: self.id,
            name: self.name,
            host: self.host,
            port: self.port,
            user: self.user,
            password: match self.password {
                Password::Keep => shellfox_ssh::PasswordUpdate::Keep,
                Password::Clear => shellfox_ssh::PasswordUpdate::Clear,
                Password::Set(v) => shellfox_ssh::PasswordUpdate::Set(v),
            },
            key_file: self.key_file,
            remote_cwd: self.remote_cwd,
        }
    }
}
