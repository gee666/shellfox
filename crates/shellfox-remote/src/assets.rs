use crate::{error, file_ops, Actor};
use shellfox_files as files;
use shellfox_protocol::{self as wire, AppError, ErrorCode, Validate};

pub(crate) fn data_url(mime: &str, bytes: &[u8]) -> Result<String, AppError> {
    let mut result = format!("data:{mime};base64,");
    for chunk in bytes.chunks(wire::FILE_CHUNK_BYTES) {
        result.push_str(
            &wire::encode_chunk(chunk)
                .map_err(|_| error(ErrorCode::Internal, "Invalid asset chunk.", false))?,
        );
    }
    Ok(result)
}
/// Preserve approved-root identity and the shared service's asset MIME/extension rules.
pub fn read(actor: &Actor, input: &wire::ReadFileAsset) -> Result<serde_json::Value, AppError> {
    input
        .validate()
        .map_err(|_| error(ErrorCode::Validation, "Invalid asset request.", false))?;
    let asset = file_ops::capability(actor, &input.path)?
        .read_asset(&files::AssetRequest {
            path: input.path.clone(),
            asset_path: input.asset_path.clone(),
        })
        .map_err(file_ops::file_error)?;
    file_ops::checked(wire::FileAsset {
        data_url: data_url(&asset.mime_type, &asset.bytes)?,
        mime_type: asset.mime_type,
        size: asset.size,
    })
}
/// Called inside the owner approval commit guard. Shared FileService validates
/// the image bytes, revision and publication, not just the declared MIME type.
pub fn write(actor: &Actor, input: &wire::WriteImageFile) -> Result<serde_json::Value, AppError> {
    input
        .validate()
        .map_err(|_| error(ErrorCode::Validation, "Invalid image write.", false))?;
    let (header, encoded) = input
        .data_url
        .split_once(',')
        .ok_or_else(|| error(ErrorCode::Validation, "Invalid image data URL.", false))?;
    let mime = header
        .strip_prefix("data:")
        .and_then(|s| s.strip_suffix(";base64"))
        .ok_or_else(|| error(ErrorCode::Validation, "Invalid image MIME type.", false))?;
    let mut bytes = Vec::new();
    for chunk in encoded.as_bytes().chunks(wire::FILE_CHUNK_BYTES / 3 * 4) {
        let chunk = std::str::from_utf8(chunk)
            .map_err(|_| error(ErrorCode::Validation, "Invalid image base64.", false))?;
        bytes.extend(
            wire::decode_chunk(chunk)
                .map_err(|_| error(ErrorCode::Validation, "Invalid image base64.", false))?,
        );
    }
    let result = file_ops::capability(actor, &input.path)?
        .write_image(&files::ImageWrite {
            path: input.path.clone(),
            revision: input.revision.clone(),
            mime_type: mime.into(),
            bytes,
        })
        .map_err(file_ops::file_error)?;
    file_ops::checked(wire::files::FileWriteResult {
        path: result.path,
        size: result.size,
        revision: result.revision,
    })
}
