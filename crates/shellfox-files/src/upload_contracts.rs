use crate::transfer_contracts::uuid_serde;
use crate::{ErrorCode, FileError, Result, TransferActor, TRANSFER_MAX_BYTES};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
use uuid::Uuid;

pub const UPLOAD_ENTRIES: usize = 128;
#[derive(Debug, Clone)]
pub struct UploadLimits {
    pub max_file_bytes: u64,
    pub active_ttl: Duration,
    pub saved_ttl: Duration,
}
impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024 * 1024,
            active_ttl: Duration::from_secs(300),
            saved_ttl: Duration::from_secs(24 * 60 * 60),
        }
    }
}
impl UploadLimits {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.max_file_bytes == 0
            || self.max_file_bytes > TRANSFER_MAX_BYTES
            || self.active_ttl.is_zero()
            || self.active_ttl > Duration::from_secs(300)
            || self.saved_ttl.is_zero()
            || self.saved_ttl > Duration::from_secs(24 * 60 * 60)
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid upload limits.",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadEnvironment {
    Local,
    Wsl { distro: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalUploadIdentity {
    #[serde(with = "uuid_serde")]
    pub tab_id: Uuid,
    #[serde(with = "uuid_serde")]
    pub generation: Uuid,
    pub control_epoch: u64,
}
/// Trusted profile context. Never deserialize this as caller-granted upload authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadActor {
    pub connection: TransferActor,
    pub terminal: TerminalUploadIdentity,
    pub environment: UploadEnvironment,
}
impl UploadActor {
    pub fn new(
        connection: TransferActor,
        terminal: TerminalUploadIdentity,
        environment: UploadEnvironment,
    ) -> Result<Self> {
        if terminal.tab_id.is_nil()
            || terminal.generation.is_nil()
            || terminal.control_epoch == 0
            || matches!(&environment, UploadEnvironment::Wsl { distro } if distro.is_empty() || distro.len() > 160 || distro.chars().any(char::is_control))
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid upload terminal identity.",
            ));
        }
        Ok(Self {
            connection,
            terminal,
            environment,
        })
    }
}
/// Must revalidate current approval, live terminal generation, control epoch,
/// and available NON-ELEVATED selected profile. Hold the host commit guard through finish.
pub trait UploadAuthorization {
    fn check(&self, actor: &UploadActor) -> Result<()>;
}
impl<F: Fn(&UploadActor) -> Result<()>> UploadAuthorization for F {
    fn check(&self, actor: &UploadActor) -> Result<()> {
        self(actor)
    }
}
pub trait UploadPathResolver {
    fn resolve(&self, native: &Path, actor: &UploadActor) -> Result<String>;
}
pub struct LocalUploadPath;
impl UploadPathResolver for LocalUploadPath {
    fn resolve(&self, native: &Path, actor: &UploadActor) -> Result<String> {
        if actor.environment != UploadEnvironment::Local {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Configure the selected WSL guest path adapter.",
            ));
        }
        crate::paths::display(native)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginUpload {
    pub file_name: String,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadIdentity {
    #[serde(with = "uuid_serde")]
    pub upload_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadChunk {
    #[serde(with = "uuid_serde")]
    pub upload_id: Uuid,
    pub offset: u64,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadStarted {
    #[serde(with = "uuid_serde")]
    pub upload_id: Uuid,
    pub chunk_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadFinished {
    pub path: String,
    pub file_name: String,
    pub size: u64,
    pub revision: String,
}

pub fn sanitize_upload_name(name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or("");
    // JS's existing sanitizer operates on UTF-16 code units, including surrogate pairs.
    let safe: String = leaf
        .encode_utf16()
        .map(|unit| {
            if unit <= 127 {
                let character = unit as u8 as char;
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ' ' | '-') {
                    return character;
                }
            }
            '_'
        })
        .take(120)
        .collect();
    let safe = safe.trim_matches([' ', '.']);
    if safe.is_empty() {
        return "file".to_owned();
    }
    let base = safe.split('.').next().unwrap_or("").to_ascii_lowercase();
    if matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && base.as_bytes()[3].is_ascii_digit())
    {
        format!("file_{safe}")
    } else {
        safe.to_owned()
    }
}
