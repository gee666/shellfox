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
    let message = if code == ErrorCode::FileConflict {
        "File changed on disk. Reload before saving."
    } else {
        "Host-account file operation failed under its OS permissions. No mutation was retried."
    };
    crate::error(code, message, false)
}
#[cfg(test)]
#[test]
fn stale_file_revisions_report_a_conflict_without_disclosing_host_details() {
    let result = file_error(files::FileError {
        code: files::ErrorCode::FileConflict,
        message: "private host path".into(),
    });
    assert_eq!(result.code, ErrorCode::FileConflict);
    assert_eq!(
        result.message,
        "File changed on disk. Reload before saving."
    );
    assert!(!result.retryable);
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
pub fn require_full_account(actor: &Actor, path: &str) -> Result<(), AppError> {
    capability(actor, path).map(|_| ())
}

/// Call while Actor::with_authorized holds the owner commit guard.
pub fn list(actor: &Actor, path: &str) -> Result<serde_json::Value, AppError> {
    let capability = capability(actor, path)?;
    let location = capability
        .browser_existing(&files::Location { path: path.into() })
        .map_err(file_error)?;
    let result = capability.list(&location).map_err(file_error)?;
    list_value(result)
}

pub fn list_value(result: files::FileList) -> Result<serde_json::Value, AppError> {
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
    let capability = capability(actor, path)?;
    let location = capability
        .browser_existing(&files::Location { path: path.into() })
        .map_err(file_error)?;
    let result = capability.read_file(&location).map_err(file_error)?;
    read_value(result)
}

pub fn read_value(result: files::ReadResult) -> Result<serde_json::Value, AppError> {
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
