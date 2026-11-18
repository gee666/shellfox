//! Remote file DTOs. A path names a file on the approved host, never on this client.
use crate::{validation::*, SessionId, TabId, Validate, ValidationError};
use serde::{Deserialize, Serialize};
const FILE_MAX_BYTES: u64 = 1024 * 1024 * 1024;

pub fn path(value: &str) -> Result<(), ValidationError> {
    text(value, 32767)?;
    let scheme = regex::Regex::new(r"(?i)^[a-z][a-z0-9+.-]+:").expect("constant regex");
    ensure(!value.is_empty() && !value.contains("://") && !scheme.is_match(value))
}
pub fn revision(value: &str) -> Result<(), ValidationError> {
    ensure(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
    )
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub session_id: Option<SessionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub tab_id: Option<TabId>,
    pub path: String,
}
impl Validate for FileLocation {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        path(&self.path)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub session_id: Option<SessionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub tab_id: Option<TabId>,
    pub path: String,
    pub text: String,
    pub revision: String,
}
impl Validate for WriteFile {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        path(&self.path)?;
        ensure(self.text.len() <= 128 * 1024 && !self.text.contains('\0'))?;
        revision(&self.revision)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileList {
    pub path: String,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub parent_path: Option<String>,
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}
impl Validate for FileList {
    fn validate(&self) -> Result<(), ValidationError> {
        path(&self.path)?;
        if let Some(parent) = &self.parent_path {
            path(parent)?;
        }
        ensure(self.entries.len() <= 500)?;
        for entry in &self.entries {
            path(&entry.name)?;
            path(&entry.path)?;
        }
        ensure(serde_json::to_vec(self).map_err(|_| ValidationError)?.len() <= 256 * 1024)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum FileRead {
    #[serde(rename_all = "camelCase")]
    Text {
        path: String,
        mime_type: String,
        size: u64,
        revision: String,
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    Image {
        path: String,
        mime_type: String,
        size: u64,
        revision: String,
        data_url: String,
    },
    #[serde(rename_all = "camelCase")]
    Binary {
        path: String,
        mime_type: String,
        size: u64,
        revision: String,
    },
}
impl Validate for FileRead {
    fn validate(&self) -> Result<(), ValidationError> {
        let (p, mime, size, rev) = match self {
            Self::Text {
                path,
                mime_type,
                size,
                revision,
                text,
            } => {
                ensure(
                    text.len() <= 8 * 1024 * 1024
                        && !text.contains('\0')
                        && text.len() as u64 == *size,
                )?;
                (path, mime_type, size, revision)
            }
            Self::Image {
                path,
                mime_type,
                size,
                revision,
                data_url,
            } => {
                ensure(matches!(
                    mime_type.as_str(),
                    "image/png"
                        | "image/jpeg"
                        | "image/gif"
                        | "image/webp"
                        | "image/x-icon"
                        | "image/avif"
                ))?;
                validate_data_url(data_url, mime_type, *size)?;
                (path, mime_type, size, revision)
            }
            Self::Binary {
                path,
                mime_type,
                size,
                revision,
            } => (path, mime_type, size, revision),
        };
        path(p)?;
        revision(rev)?;
        ensure(!mime.is_empty() && mime.encode_utf16().count() <= 100 && *size <= FILE_MAX_BYTES)
    }
}
fn validate_data_url(data: &str, mime: &str, size: u64) -> Result<(), ValidationError> {
    let prefix = format!("data:{mime};base64,");
    let encoded = data.strip_prefix(&prefix).ok_or(ValidationError)?;
    ensure(size <= 64 * 1024 * 1024 && encoded.len() % 4 == 0)?;
    let padding = if encoded.ends_with("==") {
        2
    } else if encoded.ends_with('=') {
        1
    } else {
        0
    };
    ensure(
        encoded[..encoded.len().saturating_sub(padding)]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/'),
    )?;
    if padding == 2 {
        ensure(
            encoded
                .as_bytes()
                .get(encoded.len() - 3)
                .is_some_and(|b| b"AQgw".contains(b)),
        )?;
    }
    if padding == 1 {
        ensure(
            encoded
                .as_bytes()
                .get(encoded.len() - 2)
                .is_some_and(|b| b"AEIMQUYcgkosw048".contains(b)),
        )?;
    }
    ensure((encoded.len() / 4 * 3).saturating_sub(padding) as u64 == size)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteResult {
    pub path: String,
    pub revision: String,
    pub size: u64,
}
impl Validate for FileWriteResult {
    fn validate(&self) -> Result<(), ValidationError> {
        path(&self.path)?;
        revision(&self.revision)?;
        ensure(self.size <= 64 * 1024 * 1024)
    }
}
