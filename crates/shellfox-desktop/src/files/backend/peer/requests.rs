use shellfox_protocol as wire;

pub(super) fn file_error(code: wire::ErrorCode, message: String) -> anyhow::Error {
    let code = match code {
        wire::ErrorCode::NotFound => shellfox_files::ErrorCode::NotFound,
        wire::ErrorCode::Validation => shellfox_files::ErrorCode::Validation,
        wire::ErrorCode::Unsupported => shellfox_files::ErrorCode::Unsupported,
        wire::ErrorCode::AccessDenied => shellfox_files::ErrorCode::AccessDenied,
        wire::ErrorCode::FileConflict => shellfox_files::ErrorCode::FileConflict,
        _ => shellfox_files::ErrorCode::StorageFailed,
    };
    shellfox_files::FileError { code, message }.into()
}
