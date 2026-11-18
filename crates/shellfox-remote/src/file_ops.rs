//! Fixed approved-device OS-account authority, with current approval/OS ACL checks.
use crate::{error, Actor};
use shellfox_files::{self as files, FileService};
use shellfox_protocol::{self as wire, AppError, ErrorCode, Validate};
use std::sync::Arc;

pub fn file_error(error: files::FileError) -> AppError {
    let code = match error.code {
        files::ErrorCode::Validation => ErrorCode::Validation,
        files::ErrorCode::NotFound => ErrorCode::NotFound,
        files::ErrorCode::Unsupported => ErrorCode::Unsupported,
        files::ErrorCode::AccessDenied => ErrorCode::AccessDenied,
        files::ErrorCode::FileConflict => ErrorCode::FileConflict,
        files::ErrorCode::StorageFailed => ErrorCode::StorageFailed,
    };
    crate::error(
        code,
        "Host-account file operation failed under its OS permissions. No mutation was retried.",
        false,
    )
}
pub(crate) fn capability<'a>(
    actor: &'a Actor,
    _path: &str,
) -> Result<&'a Arc<FileService>, AppError> {
    let capabilities = actor.file_capabilities();
    if capabilities.len() != 1 {
        return Err(error(
            ErrorCode::AccessDenied,
            "No explicit approved host-account capability.",
            false,
        ));
    }
    capabilities
        .first()
        .filter(|cap| cap.capability_kind() == files::CapabilityKind::ApprovedFullAccount)
        .ok_or_else(|| {
            error(
                ErrorCode::AccessDenied,
                "No explicit approved host-account capability.",
                false,
            )
        })
}
pub(crate) fn checked<T: serde::Serialize + Validate>(
    value: T,
) -> Result<serde_json::Value, AppError> {
    value
        .validate()
        .map_err(|_| error(ErrorCode::Internal, "Invalid remote file response.", false))?;
    serde_json::to_value(value).map_err(|_| {
        error(
            ErrorCode::Internal,
            "Remote file response encoding failed.",
            false,
        )
    })
}
/// Call while Actor::with_authorized holds the owner commit guard.
pub fn list(actor: &Actor, path: &str) -> Result<serde_json::Value, AppError> {
    let result = capability(actor, path)?
        .list(&files::Location { path: path.into() })
        .map_err(file_error)?;
    checked(wire::files::FileList {
        path: result.path,
        parent_path: result.parent_path,
        truncated: result.truncated,
        entries: result
            .entries
            .into_iter()
            .map(|entry| wire::files::FileEntry {
                name: entry.name,
                path: entry.path,
                kind: match entry.kind {
                    files::EntryKind::File => wire::files::EntryKind::File,
                    files::EntryKind::Directory => wire::files::EntryKind::Directory,
                    files::EntryKind::Symlink => wire::files::EntryKind::Symlink,
                    files::EntryKind::Other => wire::files::EntryKind::Other,
                },
            })
            .collect(),
    })
}
pub fn read(actor: &Actor, path: &str) -> Result<serde_json::Value, AppError> {
    let result = capability(actor, path)?
        .read_file(&files::Location { path: path.into() })
        .map_err(file_error)?;
    checked(match result {
        files::ReadResult::Text {
            path,
            mime_type,
            size,
            revision,
            text,
        } => wire::files::FileRead::Text {
            path,
            mime_type,
            size,
            revision,
            text,
        },
        files::ReadResult::Image {
            path,
            mime_type,
            size,
            revision,
            bytes,
        } => wire::files::FileRead::Image {
            path,
            data_url: crate::assets::data_url(&mime_type, &bytes)?,
            mime_type,
            size,
            revision,
        },
        files::ReadResult::Binary {
            path,
            mime_type,
            size,
            revision,
        } => wire::files::FileRead::Binary {
            path,
            mime_type,
            size,
            revision,
        },
    })
}
pub fn write(actor: &Actor, input: &wire::files::WriteFile) -> Result<serde_json::Value, AppError> {
    input
        .validate()
        .map_err(|_| error(ErrorCode::Validation, "Invalid file write.", false))?;
    let result = capability(actor, &input.path)?
        .write_text(&files::WriteText {
            path: input.path.clone(),
            text: input.text.clone(),
            revision: input.revision.clone(),
        })
        .map_err(file_error)?;
    checked(wire::files::FileWriteResult {
        path: result.path,
        revision: result.revision,
        size: result.size,
    })
}
