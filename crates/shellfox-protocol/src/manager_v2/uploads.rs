use super::*;
use crate::{validation::ensure, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalUploadStarted {
    pub upload_id: Id,
    pub chunk_bytes: usize,
}
impl Validate for TerminalUploadStarted {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.chunk_bytes == crate::FILE_CHUNK_BYTES)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeginTerminalUpload {
    pub control: ControlRef,
    pub file_name: String,
    pub size: u64,
}
impl Validate for BeginTerminalUpload {
    fn validate(&self) -> Result<(), ValidationError> {
        self.control.validate()?;
        ensure(!self.file_name.is_empty() && self.file_name.encode_utf16().count() <= 1024)?;
        crate::transfer_size(self.size)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteTerminalUpload {
    pub control: ControlRef,
    pub upload_id: Id,
    pub offset: u64,
    pub data: String,
}
impl Validate for WriteTerminalUpload {
    fn validate(&self) -> Result<(), ValidationError> {
        self.control.validate()?;
        crate::transfer_size(self.offset)?;
        crate::chunk_size(&self.data, false)?;
        Ok(())
    }
}
macro_rules! identity {
    ($name:ident) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            pub control: ControlRef,
            pub upload_id: Id,
        }
        impl Validate for $name {
            fn validate(&self) -> Result<(), ValidationError> {
                self.control.validate()
            }
        }
    };
}
identity!(FinishTerminalUpload);
identity!(CancelTerminalUpload);
