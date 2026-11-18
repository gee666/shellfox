use crate::*;
use serde::{Deserialize, Serialize};

macro_rules! commands {
    ($($name:ident($ty:ty) = $wire:literal),+ $(,)?) => {
        /// Typed remote allowlist only. Local manager privileges cannot be represented.
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(tag = "method", content = "payload", deny_unknown_fields)]
        pub enum RemoteCommand { $(#[serde(rename = $wire)] $name($ty)),+ }
        pub const REMOTE_METHODS: &[&str] = &[$($wire),+];
        impl RemoteCommand {
            pub fn method_name(&self) -> &'static str { match self { $(Self::$name(_) => $wire),+ } }
        }
        impl Validate for RemoteCommand {
            fn validate(&self) -> Result<(), ValidationError> {
                match self { $(Self::$name(value) => value.validate()),+ }
            }
        }
    };
}
commands! {
    GetSnapshot(GetSnapshot) = "getSnapshot",
    GetTerminalProfiles(GetTerminalProfiles) = "getTerminalProfiles",
    GetHomeDirectory(GetHomeDirectory) = "getHomeDirectory",
    CreateSession(CreateSession) = "createSession",
    AddTab(AddTab) = "addTab",
    ActivateSession(ActivateSession) = "activateSession",
    RenameSession(RenameSession) = "renameSession",
    RenameTab(RenameTab) = "renameTab",
    ReorderTabs(ReorderTabs) = "reorderTabs",
    SetSessionPinned(SetSessionPinned) = "setSessionPinned",
    SettleSession(SettleSession) = "settleSession",
    UnsettleSession(UnsettleSession) = "unsettleSession",
    DeleteSession(DeleteSession) = "deleteSession",
    ClearSessionError(ClearSessionError) = "clearSessionError",
    SetSessionEnv(SetSessionEnv) = "setSessionEnv",
    SetTerminalEnv(SetTerminalEnv) = "setTerminalEnv",
    SetVisibleSessions(SetVisibleSessions) = "setVisibleSessions",
    AttachTerminal(AttachTerminal) = "attachTerminal",
    TakeTerminalControl(TakeTerminalControl) = "takeTerminalControl",
    WriteTerminal(WriteTerminal) = "writeTerminal",
    ResizeTerminal(ResizeTerminal) = "resizeTerminal",
    AcknowledgeTerminal(AcknowledgeTerminal) = "acknowledgeTerminal",
    DetachTerminal(DetachTerminal) = "detachTerminal",
    CloseTab(CloseTab) = "closeTab",
    ListFiles(ListFiles) = "listFiles",
    ReadFile(ReadFile) = "readFile",
    WriteFile(files::WriteFile) = "writeFile",
    BeginFileRead(BeginFileRead) = "beginFileRead",
    ReadFileChunk(ReadFileChunk) = "readFileChunk",
    BeginFileWrite(BeginFileWrite) = "beginFileWrite",
    WriteFileChunk(WriteFileChunk) = "writeFileChunk",
    FinishFileWrite(FinishFileWrite) = "finishFileWrite",
    CancelFileTransfer(CancelFileTransfer) = "cancelFileTransfer",
    WriteImageFile(WriteImageFile) = "writeImageFile",
    ReadFileAsset(ReadFileAsset) = "readFileAsset",
    GetSessionStats(GetSessionStats) = "getSessionStats",
    ResolveDirectory(ResolveDirectory) = "resolveDirectory",
    CreateDirectory(CreateDirectory) = "createDirectory",
    CompleteDirectory(CompleteDirectory) = "completeDirectory",
    BeginTerminalUpload(BeginTerminalUpload) = "beginTerminalUpload",
    WriteTerminalUpload(WriteTerminalUpload) = "writeTerminalUpload",
    FinishTerminalUpload(FinishTerminalUpload) = "finishTerminalUpload",
    CancelTerminalUpload(CancelTerminalUpload) = "cancelTerminalUpload",
}
