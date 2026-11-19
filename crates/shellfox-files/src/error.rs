use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Validation,
    NotFound,
    Unsupported,
    AccessDenied,
    FileConflict,
    StorageFailed,
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[error("{message}")]
pub struct FileError {
    pub code: ErrorCode,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, FileError>;

impl FileError {
    pub(crate) fn new(code: ErrorCode, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
        }
    }
    pub(crate) fn denied() -> Self {
        Self::new(ErrorCode::AccessDenied, "File access was denied.")
    }
    pub(crate) fn conflict() -> Self {
        Self::new(
            ErrorCode::FileConflict,
            "File changed on disk. Reload before saving.",
        )
    }
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        use io::ErrorKind::*;
        #[cfg(unix)]
        if error.raw_os_error() == Some(libc::ELOOP) {
            return Self::denied();
        }
        match error.kind() {
            NotFound | NotADirectory => {
                Self::new(ErrorCode::NotFound, "File or directory not found.")
            }
            PermissionDenied => Self::denied(),
            AlreadyExists | DirectoryNotEmpty => Self::conflict(),
            InvalidInput | InvalidFilename => {
                Self::new(ErrorCode::Validation, "Invalid file path or operation.")
            }
            _ => Self::new(
                ErrorCode::StorageFailed,
                "File operation failed. No uncertain operation was retried.",
            ),
        }
    }
}
