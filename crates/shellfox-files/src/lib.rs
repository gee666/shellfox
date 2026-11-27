//! Local file operations scoped to an approved root or the local desktop account.
//! No network transport: adapters own authentication, approval and wire encoding.
mod assets;
mod browser_wsl;
mod content;
mod contracts;
mod directory_ops;
mod directory_wsl;
mod editor;
mod error;
pub mod handle;
mod image_save;
mod mutations;
mod paths;
mod permissions;
mod platform;
pub mod private;
mod read_result;
mod service;
mod staged_write;
mod stream_io;
#[cfg(test)]
mod test_support;
mod transfer_begin;
mod transfer_chunks;
mod transfer_contracts;
mod transfer_execute;
mod transfer_registry;
mod upload_contracts;
mod upload_inbox;
mod upload_storage;
mod upload_transfer;
pub mod wsl_files_ipc;

pub use assets::{AssetRequest, FileAsset, ImageWrite, INLINE_IMAGE_WRITE_BYTES};
pub use browser_wsl::WslBrowserScope;
pub use contracts::*;
pub use directory_ops::{
    DirectoryCompletions, DirectoryResolved, FileContext, DIRECTORY_COMPLETION_BYTES,
    DIRECTORY_COMPLETION_LIMIT,
};
#[cfg(target_os = "linux")]
pub use editor::serve_wsl_files;
pub use editor::{
    EditorBackend, EditorCancellation, EditorFileSink, EditorFileSource, EditorSession,
};
pub use error::{ErrorCode, FileError, Result};
pub use handle::HandleIdentity;
pub use read_result::ReadResult;
pub use service::{FileService, NativeCommitFence};
pub use transfer_contracts::*;
pub use transfer_execute::{TransferReply, TransferRequest};
pub use transfer_registry::{TransferRegistry, TransferUsage};
pub use upload_contracts::*;
pub use upload_inbox::{UploadInbox, UploadUsage};
