use crate::{FileService, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

pub(crate) mod uuid_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;
    pub fn serialize<S: Serializer>(id: &Uuid, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&id.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
        let value = String::deserialize(deserializer)?;
        Uuid::parse_str(&value).map_err(serde::de::Error::custom)
    }
}

pub const CHUNK_BYTES: usize = 192 * 1024;
pub const TRANSFER_MAX_BYTES: u64 = 1024 * 1024 * 1024;
pub const TRANSFER_TEXT_BYTES: u64 = 8 * 1024 * 1024;
pub const TRANSFER_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
pub const TRANSFERS_PER_ACTOR: usize = 2;
pub const TRANSFERS_TOTAL: usize = 8;
pub const TRANSFER_RECEIPTS: usize = 128;

#[derive(Debug, Clone)]
pub struct TransferLimits {
    pub max_file_bytes: u64,
    pub idle_ttl: Duration,
}
impl Default for TransferLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024 * 1024,
            idle_ttl: Duration::from_secs(300),
        }
    }
}

/// Trusted connection identity, never deserialized from a transfer request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferActor {
    pub(crate) owner: String,
    pub(crate) generation: Uuid,
}
impl TransferActor {
    pub fn new(owner: String, generation: Uuid) -> Result<Self> {
        if owner.is_empty()
            || owner.len() > 256
            || owner.chars().any(char::is_control)
            || generation.is_nil()
        {
            return Err(crate::FileError::new(
                crate::ErrorCode::Validation,
                "Invalid transfer actor.",
            ));
        }
        Ok(Self { owner, generation })
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn generation(&self) -> Uuid {
        self.generation
    }
}

/// The caller holds its approval/control commit guard while this check runs.
/// Implementations must consult live policy, not a boolean captured at begin.
pub trait TransferAuthorization {
    fn check(&self, actor: &TransferActor, capability: &FileService, write: bool) -> Result<()>;
}
impl<F> TransferAuthorization for F
where
    F: Fn(&TransferActor, &FileService, bool) -> Result<()>,
{
    fn check(&self, actor: &TransferActor, capability: &FileService, write: bool) -> Result<()> {
        self(actor, capability, write)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginRead {
    pub path: String,
    pub asset_path: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    Create,
    Replace,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferContent {
    File,
    Text,
    Image,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginWrite {
    pub path: String,
    pub size: u64,
    pub mode: WriteMode,
    pub revision: Option<String>,
    pub content: TransferContent,
    pub mime_type: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferIdentity {
    #[serde(with = "uuid_serde")]
    pub transfer_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadChunk {
    #[serde(with = "uuid_serde")]
    pub transfer_id: Uuid,
    pub offset: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteChunk {
    #[serde(with = "uuid_serde")]
    pub transfer_id: Uuid,
    pub offset: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text,
    Image,
    Binary,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadStarted {
    #[serde(with = "uuid_serde")]
    pub transfer_id: Uuid,
    pub path: String,
    pub size: u64,
    pub revision: String,
    pub kind: ContentKind,
    pub mime_type: String,
    pub chunk_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteStarted {
    #[serde(with = "uuid_serde")]
    pub transfer_id: Uuid,
    pub chunk_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadChunkResult {
    pub offset: u64,
    pub data: Vec<u8>,
    pub eof: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub offset: u64,
}
