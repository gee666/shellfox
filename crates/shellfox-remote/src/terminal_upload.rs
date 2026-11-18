//! Existing private inbox engine for the privileged Service owner adapter.
pub use shellfox_files::{
    BeginUpload, ErrorCode, FileError, LocalUploadPath, Result, TerminalUploadIdentity,
    TransferActor, UploadActor, UploadChunk, UploadEnvironment, UploadIdentity, UploadInbox,
    UploadLimits,
};
