use crate::{validation::ensure, *};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileReadKind {
    Text,
    Image,
    Binary,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileReadTransfer {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub transfer_id: TransferId,
    pub path: String,
    pub size: u64,
    pub revision: String,
    pub kind: FileReadKind,
    pub mime_type: String,
    pub chunk_bytes: usize,
}
impl Validate for FileReadTransfer {
    fn validate(&self) -> Result<(), ValidationError> {
        files::path(&self.path)?;
        transfer_size(self.size)?;
        files::revision(&self.revision)?;
        ensure(
            !self.mime_type.is_empty()
                && self.mime_type.encode_utf16().count() <= 100
                && self.chunk_bytes == FILE_CHUNK_BYTES,
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileWriteTransfer {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub transfer_id: TransferId,
    pub chunk_bytes: usize,
}
impl Validate for FileWriteTransfer {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.chunk_bytes == FILE_CHUNK_BYTES)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileChunk {
    pub offset: u64,
    pub data: String,
    pub eof: bool,
}
impl Validate for FileChunk {
    fn validate(&self) -> Result<(), ValidationError> {
        transfer_size(self.offset)?;
        chunk_size(&self.data, true)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferOffset {
    pub offset: u64,
}
impl Validate for TransferOffset {
    fn validate(&self) -> Result<(), ValidationError> {
        transfer_size(self.offset)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileTransferResult {
    pub path: String,
    pub size: u64,
    pub revision: String,
}
impl Validate for FileTransferResult {
    fn validate(&self) -> Result<(), ValidationError> {
        files::path(&self.path)?;
        transfer_size(self.size)?;
        files::revision(&self.revision)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cancelled {
    pub cancelled: bool,
}
impl Validate for Cancelled {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.cancelled)
    }
}
