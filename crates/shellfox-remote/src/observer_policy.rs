use crate::{error, ViewPermission};
use shellfox_protocol::{observer_v2 as wire, AppError, ErrorCode, RemoteCommand, Validate};

/// Existing per-method approval policy, with observer delivery's read-only
/// Attach/Ack/Detach exception. Every other payload keeps its exact v1 authority.
pub fn authorize(permission: ViewPermission, command: &wire::Command) -> Result<(), AppError> {
    command
        .validate()
        .map_err(|_| error(ErrorCode::Validation, "Invalid observer request.", false))?;
    command.authorize_profile().map_err(|failure| AppError {
        code: failure.code,
        message: failure.message,
        retryable: failure.retryable,
    })?;
    if let Some(legacy) = legacy_command(command) {
        crate::policy::authorize(permission, &legacy)?;
    }
    Ok(())
}
macro_rules! legacy {
    ($command:expr,$($name:ident),+)=> {match $command {
        $(wire::Command::$name(value)=>Some(RemoteCommand::$name(value.0.clone())),)+
        wire::Command::AttachTerminal(_)|wire::Command::AcknowledgeTerminal(_)|wire::Command::DetachTerminal(_)=>None,
    }};
}
pub fn legacy_command(command: &wire::Command) -> Option<RemoteCommand> {
    legacy!(
        command,
        GetSnapshot,
        GetTerminalProfiles,
        GetHomeDirectory,
        CreateSession,
        AddTab,
        ActivateSession,
        RenameSession,
        RenameTab,
        ReorderTabs,
        SetSessionPinned,
        SettleSession,
        UnsettleSession,
        DeleteSession,
        ClearSessionError,
        SetSessionEnv,
        SetTerminalEnv,
        SetVisibleSessions,
        TakeTerminalControl,
        WriteTerminal,
        ResizeTerminal,
        CloseTab,
        ListFiles,
        ReadFile,
        WriteFile,
        BeginFileRead,
        ReadFileChunk,
        BeginFileWrite,
        WriteFileChunk,
        FinishFileWrite,
        CancelFileTransfer,
        WriteImageFile,
        ReadFileAsset,
        GetSessionStats,
        ResolveDirectory,
        CreateDirectory,
        CompleteDirectory,
        BeginTerminalUpload,
        WriteTerminalUpload,
        FinishTerminalUpload,
        CancelTerminalUpload
    )
}
/// Validate the actual typed method result before it reaches a canonical bound
/// reply. Value is the existing owner-channel carrier, never a request escape.
pub(crate) fn validate_result(
    command: &wire::Command,
    value: &serde_json::Value,
) -> Result<(), AppError> {
    fn response<R: wire::Request>(value: &serde_json::Value) -> Result<(), AppError> {
        let response: R::Response = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        response.validate().map_err(|_| invalid())
    }
    macro_rules! responses {($($name:ident),+)=>{match command {$(wire::Command::$name(_)=>response::<wire::$name>(value)),+}};}
    responses!(
        GetSnapshot,
        GetTerminalProfiles,
        GetHomeDirectory,
        CreateSession,
        AddTab,
        ActivateSession,
        RenameSession,
        RenameTab,
        ReorderTabs,
        SetSessionPinned,
        SettleSession,
        UnsettleSession,
        DeleteSession,
        ClearSessionError,
        SetSessionEnv,
        SetTerminalEnv,
        SetVisibleSessions,
        AttachTerminal,
        TakeTerminalControl,
        WriteTerminal,
        ResizeTerminal,
        AcknowledgeTerminal,
        DetachTerminal,
        CloseTab,
        ListFiles,
        ReadFile,
        WriteFile,
        BeginFileRead,
        ReadFileChunk,
        BeginFileWrite,
        WriteFileChunk,
        FinishFileWrite,
        CancelFileTransfer,
        WriteImageFile,
        ReadFileAsset,
        GetSessionStats,
        ResolveDirectory,
        CreateDirectory,
        CompleteDirectory,
        BeginTerminalUpload,
        WriteTerminalUpload,
        FinishTerminalUpload,
        CancelTerminalUpload
    )
}
fn invalid() -> AppError {
    error(
        ErrorCode::Internal,
        "Owner returned an invalid observer-v2 method result.",
        false,
    )
}
