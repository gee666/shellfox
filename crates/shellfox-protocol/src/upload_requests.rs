use crate::{validation::*, *};
use serde::{Deserialize, Serialize};
pub type UploadId = ClientId;

macro_rules! upload_method {
    ($ty:ty,$reply:ty,$name:literal) => {
        impl crate::methods::private::Sealed for $ty {}
        impl Request for $ty {
            type Response = $reply;
            const METHOD: &'static str = $name;
            const LEGACY_UPLOAD: bool = true;
        }
    };
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeginTerminalUpload {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub control_epoch: u64,
    pub file_name: String,
    pub size: u64,
}
impl Validate for BeginTerminalUpload {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.control_epoch)?;
        transfer_size(self.size)?;
        upload_name(&self.file_name)
    }
}
upload_method!(
    BeginTerminalUpload,
    TerminalUploadStarted,
    "beginTerminalUpload"
);
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteTerminalUpload {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub control_epoch: u64,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub upload_id: UploadId,
    pub offset: u64,
    pub data: String,
}
impl Validate for WriteTerminalUpload {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.control_epoch)?;
        transfer_size(self.offset)?;
        chunk_size(&self.data, false)?;
        Ok(())
    }
}
upload_method!(WriteTerminalUpload, TransferOffset, "writeTerminalUpload");
macro_rules! upload_identity {
    ($ty:ident,$reply:ty,$method:literal) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $ty {
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub tab_id: TabId,
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub generation: Generation,
            pub control_epoch: u64,
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub upload_id: UploadId,
        }
        impl Validate for $ty {
            fn validate(&self) -> Result<(), ValidationError> {
                epoch(self.control_epoch)
            }
        }
        upload_method!($ty, $reply, $method);
    };
}
upload_identity!(
    FinishTerminalUpload,
    TerminalUploadResult,
    "finishTerminalUpload"
);
upload_identity!(CancelTerminalUpload, Cancelled, "cancelTerminalUpload");
pub(crate) fn upload_name(value: &str) -> Result<(), ValidationError> {
    ensure(!value.is_empty() && value.encode_utf16().count() <= 1024)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalUploadStarted {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub upload_id: UploadId,
    pub chunk_bytes: usize,
}
impl Validate for TerminalUploadStarted {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.chunk_bytes == FILE_CHUNK_BYTES)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalUploadResult {
    pub path: String,
    pub file_name: String,
    pub size: u64,
}
impl Validate for TerminalUploadResult {
    fn validate(&self) -> Result<(), ValidationError> {
        cwd(&self.path)?;
        upload_name(&self.file_name)?;
        transfer_size(self.size)
    }
}
