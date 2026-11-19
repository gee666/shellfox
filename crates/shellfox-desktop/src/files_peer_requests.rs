use shellfox_protocol::{self as wire, manager_v2 as m};

pub(super) trait FileRequest: wire::Request + Clone {
    type Manager: m::Request<Response = <Self as wire::Request>::Response>;
    fn manager(&self) -> Self::Manager;
}

macro_rules! compatible_files {
    ($($name:ident),+ $(,)?) => { $(
        impl FileRequest for wire::$name {
            type Manager = m::$name;
            fn manager(&self) -> Self::Manager { m::$name(self.clone()) }
        }
    )+ };
}
compatible_files!(GetHomeDirectory, CompleteDirectory, CreateDirectory, ListFiles, BeginFileRead,
    ReadFileChunk, BeginFileWrite, WriteFileChunk, FinishFileWrite, CancelFileTransfer);

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
