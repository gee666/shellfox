use crate::{methods::method, validation::ensure, *};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteImageFile {
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
    pub revision: String,
    pub data_url: String,
}
impl Validate for WriteImageFile {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        files::path(&self.path)?;
        files::revision(&self.revision)?;
        ensure(self.data_url.len() <= (512_usize * 1024).div_ceil(3) * 4 + 128)?;
        let (header, data) = self.data_url.split_once(',').ok_or(ValidationError)?;
        ensure(matches!(
            header,
            "data:image/png;base64" | "data:image/jpeg;base64" | "data:image/webp;base64"
        ))?;
        let bytes = crate::chunks::base64_size(data)?;
        ensure(bytes > 0 && bytes <= 512 * 1024)
    }
}
method!(WriteImageFile, files::FileWriteResult, "writeImageFile");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadFileAsset {
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
    pub asset_path: String,
}
impl Validate for ReadFileAsset {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        files::path(&self.path)?;
        files::path(&self.asset_path)
    }
}
method!(ReadFileAsset, FileAsset, "readFileAsset");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileAsset {
    pub mime_type: String,
    pub data_url: String,
    pub size: u64,
}
impl Validate for FileAsset {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(matches!(
            self.mime_type.as_str(),
            "image/png"
                | "image/jpeg"
                | "image/gif"
                | "image/webp"
                | "image/x-icon"
                | "image/avif"
                | "text/css"
                | "text/javascript"
                | "application/json"
                | "image/svg+xml"
                | "font/woff"
                | "font/woff2"
                | "font/ttf"
                | "font/otf"
        ))?;
        ensure(
            self.size <= 64 * 1024 * 1024
                && self.data_url.len() <= (64_usize * 1024 * 1024).div_ceil(3) * 4 + 128,
        )?;
        let prefix = format!("data:{};base64,", self.mime_type);
        let data = self.data_url.strip_prefix(&prefix).ok_or(ValidationError)?;
        ensure(crate::chunks::base64_size(data)? as u64 == self.size)
    }
}
