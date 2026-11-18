use crate::{methods::method, validation::ensure, *};
use serde::{Deserialize, Serialize};

pub type TransferId = ClientId;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeginFileRead {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::ids::optional_wire_id"
    )]
    pub session_id: Option<SessionId>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::ids::optional_wire_id"
    )]
    pub tab_id: Option<TabId>,
    pub path: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::optional"
    )]
    pub asset_path: Option<String>,
}
impl Validate for BeginFileRead {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        files::path(&self.path)?;
        if let Some(path) = &self.asset_path {
            files::path(path)?;
        }
        Ok(())
    }
}
method!(BeginFileRead, FileReadTransfer, "beginFileRead");
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileWriteMode {
    Create,
    Replace,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileContent {
    File,
    Text,
    Image,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ImageWriteMime {
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/webp")]
    Webp,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeginFileWrite {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::ids::optional_wire_id"
    )]
    pub session_id: Option<SessionId>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::ids::optional_wire_id"
    )]
    pub tab_id: Option<TabId>,
    pub path: String,
    pub size: u64,
    pub mode: FileWriteMode,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::optional"
    )]
    pub revision: Option<String>,
    pub content: FileContent,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::optional"
    )]
    pub mime_type: Option<ImageWriteMime>,
}
impl Validate for BeginFileWrite {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        files::path(&self.path)?;
        transfer_size(self.size)?;
        if let Some(value) = &self.revision {
            files::revision(value)?;
        }
        ensure(
            (self.mode == FileWriteMode::Replace) == self.revision.is_some()
                && (self.content == FileContent::Image) == self.mime_type.is_some(),
        )
    }
}
method!(BeginFileWrite, FileWriteTransfer, "beginFileWrite");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadFileChunk {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub transfer_id: TransferId,
    pub offset: u64,
}
impl Validate for ReadFileChunk {
    fn validate(&self) -> Result<(), ValidationError> {
        transfer_size(self.offset)
    }
}
method!(ReadFileChunk, FileChunk, "readFileChunk");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteFileChunk {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub transfer_id: TransferId,
    pub offset: u64,
    pub data: String,
}
impl Validate for WriteFileChunk {
    fn validate(&self) -> Result<(), ValidationError> {
        transfer_size(self.offset)?;
        chunk_size(&self.data, false)?;
        Ok(())
    }
}
method!(WriteFileChunk, TransferOffset, "writeFileChunk");
macro_rules! identity {
    ($ty:ident,$reply:ty,$method:literal) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $ty {
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub transfer_id: TransferId,
        }
        impl Validate for $ty {
            fn validate(&self) -> Result<(), ValidationError> {
                Ok(())
            }
        }
        method!($ty, $reply, $method);
    };
}
identity!(FinishFileWrite, FileTransferResult, "finishFileWrite");
identity!(CancelFileTransfer, Cancelled, "cancelFileTransfer");
