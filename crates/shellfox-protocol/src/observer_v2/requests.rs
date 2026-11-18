use super::*;
use crate::{validation::ensure, Validate, ValidationError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

mod sealed {
    pub trait Sealed {}
}
pub trait Request: sealed::Sealed + Serialize + DeserializeOwned + Validate {
    type Response: Serialize + DeserializeOwned + Validate;
    const METHOD: &'static str;
    const ALLOWED: bool;
    fn validate_wire(&self) -> Result<(), ValidationError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| ValidationError)?;
        ensure(bytes.len() <= crate::MAX_REQUEST_BYTES)?;
        let _: Self = serde_json::from_slice(&bytes).map_err(|_| ValidationError)?;
        Ok(())
    }
}
macro_rules! adapt {
    ($name:ident,$v1:ty,$reply:ty,$method:literal,$allowed:literal) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $v1);
        impl From<$v1> for $name {
            fn from(value: $v1) -> Self {
                Self(value)
            }
        }
        impl Validate for $name {
            fn validate(&self) -> Result<(), ValidationError> {
                crate::Request::validate_wire(&self.0)
            }
        }
        impl sealed::Sealed for $name {}
        impl Request for $name {
            type Response = $reply;
            const METHOD: &'static str = $method;
            const ALLOWED: bool = $allowed;
        }
    };
}
adapt!(
    GetSnapshot,
    crate::GetSnapshot,
    Snapshot,
    "getSnapshot",
    true
);
adapt!(
    GetTerminalProfiles,
    crate::GetTerminalProfiles,
    crate::TerminalProfiles,
    "getTerminalProfiles",
    true
);
adapt!(
    GetHomeDirectory,
    crate::GetHomeDirectory,
    crate::Directory,
    "getHomeDirectory",
    true
);
adapt!(
    CreateSession,
    crate::CreateSession,
    CatalogEntry,
    "createSession",
    true
);
adapt!(AddTab, crate::AddTab, CatalogEntry, "addTab", true);
adapt!(
    ActivateSession,
    crate::ActivateSession,
    CatalogEntry,
    "activateSession",
    true
);
adapt!(
    RenameSession,
    crate::RenameSession,
    CatalogEntry,
    "renameSession",
    true
);
adapt!(RenameTab, crate::RenameTab, CatalogEntry, "renameTab", true);
adapt!(
    ReorderTabs,
    crate::ReorderTabs,
    CatalogEntry,
    "reorderTabs",
    true
);
adapt!(
    SetSessionPinned,
    crate::SetSessionPinned,
    CatalogEntry,
    "setSessionPinned",
    true
);
adapt!(
    SettleSession,
    crate::SettleSession,
    CatalogEntry,
    "settleSession",
    true
);
adapt!(
    UnsettleSession,
    crate::UnsettleSession,
    CatalogEntry,
    "unsettleSession",
    true
);
adapt!(
    DeleteSession,
    crate::DeleteSession,
    crate::Deleted,
    "deleteSession",
    true
);
adapt!(
    ClearSessionError,
    crate::ClearSessionError,
    CatalogEntry,
    "clearSessionError",
    true
);
adapt!(
    SetSessionEnv,
    crate::SetSessionEnv,
    CatalogEntry,
    "setSessionEnv",
    true
);
adapt!(
    SetTerminalEnv,
    crate::SetTerminalEnv,
    Settings,
    "setTerminalEnv",
    true
);
adapt!(
    SetVisibleSessions,
    crate::SetVisibleSessions,
    crate::Configured,
    "setVisibleSessions",
    true
);
adapt!(
    TakeTerminalControl,
    crate::TakeTerminalControl,
    crate::Taken,
    "takeTerminalControl",
    false
);
adapt!(
    WriteTerminal,
    crate::WriteTerminal,
    crate::Written,
    "writeTerminal",
    false
);
adapt!(
    ResizeTerminal,
    crate::ResizeTerminal,
    crate::Resized,
    "resizeTerminal",
    false
);
adapt!(CloseTab, crate::CloseTab, CatalogEntry, "closeTab", true);
adapt!(
    ListFiles,
    crate::ListFiles,
    crate::files::FileList,
    "listFiles",
    true
);
adapt!(
    ReadFile,
    crate::ReadFile,
    crate::files::FileRead,
    "readFile",
    true
);
adapt!(
    WriteFile,
    crate::files::WriteFile,
    crate::files::FileWriteResult,
    "writeFile",
    true
);
adapt!(
    BeginFileRead,
    crate::BeginFileRead,
    crate::FileReadTransfer,
    "beginFileRead",
    true
);
adapt!(
    ReadFileChunk,
    crate::ReadFileChunk,
    crate::FileChunk,
    "readFileChunk",
    true
);
adapt!(
    BeginFileWrite,
    crate::BeginFileWrite,
    crate::FileWriteTransfer,
    "beginFileWrite",
    true
);
adapt!(
    WriteFileChunk,
    crate::WriteFileChunk,
    crate::TransferOffset,
    "writeFileChunk",
    true
);
adapt!(
    FinishFileWrite,
    crate::FinishFileWrite,
    crate::FileTransferResult,
    "finishFileWrite",
    true
);
adapt!(
    CancelFileTransfer,
    crate::CancelFileTransfer,
    crate::Cancelled,
    "cancelFileTransfer",
    true
);
adapt!(
    WriteImageFile,
    crate::WriteImageFile,
    crate::files::FileWriteResult,
    "writeImageFile",
    true
);
adapt!(
    ReadFileAsset,
    crate::ReadFileAsset,
    crate::FileAsset,
    "readFileAsset",
    true
);
adapt!(
    GetSessionStats,
    crate::GetSessionStats,
    SessionStats,
    "getSessionStats",
    true
);
adapt!(
    ResolveDirectory,
    crate::ResolveDirectory,
    crate::Directory,
    "resolveDirectory",
    true
);
adapt!(
    CreateDirectory,
    crate::CreateDirectory,
    crate::Directory,
    "createDirectory",
    true
);
adapt!(
    CompleteDirectory,
    crate::CompleteDirectory,
    crate::DirectoryMatches,
    "completeDirectory",
    true
);
adapt!(
    BeginTerminalUpload,
    crate::BeginTerminalUpload,
    crate::TerminalUploadStarted,
    "beginTerminalUpload",
    false
);
adapt!(
    WriteTerminalUpload,
    crate::WriteTerminalUpload,
    crate::TransferOffset,
    "writeTerminalUpload",
    false
);
adapt!(
    FinishTerminalUpload,
    crate::FinishTerminalUpload,
    crate::TerminalUploadResult,
    "finishTerminalUpload",
    false
);
adapt!(
    CancelTerminalUpload,
    crate::CancelTerminalUpload,
    crate::Cancelled,
    "cancelTerminalUpload",
    false
);
macro_rules! direct {
    ($name:ident,$reply:ty,$method:literal) => {
        impl sealed::Sealed for $name {}
        impl Request for $name {
            type Response = $reply;
            const METHOD: &'static str = $method;
            const ALLOWED: bool = true;
        }
    };
}
direct!(AttachTerminal, Attachment, "attachTerminal");
direct!(
    AcknowledgeTerminal,
    crate::Acknowledged,
    "acknowledgeTerminal"
);
direct!(DetachTerminal, crate::Detached, "detachTerminal");
