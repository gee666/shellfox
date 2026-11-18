use crate::{
    content,
    paths::{self, Target},
    ErrorCode, FileError, FileService, Result, PREVIEW_BYTES,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRequest {
    pub path: String,
    pub asset_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAsset {
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageWrite {
    pub path: String,
    pub revision: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}
pub const INLINE_IMAGE_WRITE_BYTES: usize = 512 * 1024;

pub(crate) fn mime(path: &Path) -> &'static str {
    match extension(path).as_str() {
        "html" | "htm" | "xhtml" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "md" | "mdx" | "markdown" | "mdown" | "mkd" => "text/markdown",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "text/plain",
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
pub(crate) fn editable_image(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/webp")
}
pub(crate) fn asset_target(
    service: &FileService,
    input: &AssetRequest,
) -> Result<(Target, &'static str)> {
    paths::validate(&input.asset_path)?;
    let asset = Path::new(&input.asset_path);
    if asset.is_absolute()
        || input.asset_path.starts_with(['/', '\\'])
        || input.asset_path.contains(['?', '#', ':'])
    {
        return Err(FileError::denied());
    }
    let base = service.target(&input.path)?;
    if !matches!(
        extension(&base.absolute).as_str(),
        "html" | "htm" | "xhtml" | "md" | "mdx" | "markdown" | "mdown" | "mkd"
    ) {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Preview assets require an HTML or Markdown file.",
        ));
    }
    content::open_regular(&base.parent, &base.name)?;
    if !matches!(
        extension(asset).as_str(),
        "css"
            | "js"
            | "mjs"
            | "json"
            | "svg"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "avif"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
    ) {
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "This file type cannot be embedded as a preview asset.",
        ));
    }
    let parent = paths::descend(
        base.parent.try_clone()?,
        asset.parent().unwrap_or(Path::new("")),
    )?;
    let absolute = base
        .absolute
        .parent()
        .ok_or_else(FileError::denied)?
        .join(asset);
    let mime = mime(&absolute);
    Ok((
        Target {
            parent,
            name: PathBuf::from(asset.file_name().ok_or_else(FileError::denied)?),
            absolute,
        },
        mime,
    ))
}

impl FileService {
    pub fn read_asset(&self, input: &AssetRequest) -> Result<FileAsset> {
        let (target, mime_type) = asset_target(self, input)?;
        let (bytes, _) = content::read(
            &mut content::open_regular(&target.parent, &target.name)?,
            PREVIEW_BYTES,
        )?;
        self.authorized(false)?;
        Ok(FileAsset {
            size: bytes.len() as u64,
            bytes,
            mime_type: mime_type.to_owned(),
        })
    }
}
