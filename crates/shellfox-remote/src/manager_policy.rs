use crate::{error, Actor, ViewPermission};
use shellfox_protocol::{
    manager_v2 as wire, observer_v2 as observer, AppError, ErrorCode, Validate,
};
pub fn compatible(command: &wire::Command) -> Option<observer::Command> {
    match command {
        wire::Command::GetSnapshot(value) => Some(observer::Command::GetSnapshot(value.clone())),
        wire::Command::GetTerminalProfiles(value) => {
            Some(observer::Command::GetTerminalProfiles(value.clone()))
        }
        wire::Command::GetHomeDirectory(value) => {
            Some(observer::Command::GetHomeDirectory(value.clone()))
        }
        wire::Command::CreateSession(value) => {
            Some(observer::Command::CreateSession(value.clone()))
        }
        wire::Command::AddTab(value) => Some(observer::Command::AddTab(value.clone())),
        wire::Command::ActivateSession(value) => {
            Some(observer::Command::ActivateSession(value.clone()))
        }
        wire::Command::RenameSession(value) => {
            Some(observer::Command::RenameSession(value.clone()))
        }
        wire::Command::RenameTab(value) => Some(observer::Command::RenameTab(value.clone())),
        wire::Command::ReorderTabs(value) => Some(observer::Command::ReorderTabs(value.clone())),
        wire::Command::SetSessionPinned(value) => {
            Some(observer::Command::SetSessionPinned(value.clone()))
        }
        wire::Command::SettleSession(value) => {
            Some(observer::Command::SettleSession(value.clone()))
        }
        wire::Command::UnsettleSession(value) => {
            Some(observer::Command::UnsettleSession(value.clone()))
        }
        wire::Command::DeleteSession(value) => {
            Some(observer::Command::DeleteSession(value.clone()))
        }
        wire::Command::ClearSessionError(value) => {
            Some(observer::Command::ClearSessionError(value.clone()))
        }
        wire::Command::SetSessionEnv(value) => {
            Some(observer::Command::SetSessionEnv(value.clone()))
        }
        wire::Command::SetTerminalEnv(value) => {
            Some(observer::Command::SetTerminalEnv(value.clone()))
        }
        wire::Command::SetVisibleSessions(value) => {
            Some(observer::Command::SetVisibleSessions(value.clone()))
        }
        wire::Command::CloseTab(value) => Some(observer::Command::CloseTab(value.clone())),
        wire::Command::ListFiles(value) => Some(observer::Command::ListFiles(value.clone())),
        wire::Command::ReadFile(value) => Some(observer::Command::ReadFile(value.clone())),
        wire::Command::WriteFile(value) => Some(observer::Command::WriteFile(value.clone())),
        wire::Command::BeginFileRead(value) => {
            Some(observer::Command::BeginFileRead(value.clone()))
        }
        wire::Command::ReadFileChunk(value) => {
            Some(observer::Command::ReadFileChunk(value.clone()))
        }
        wire::Command::BeginFileWrite(value) => {
            Some(observer::Command::BeginFileWrite(value.clone()))
        }
        wire::Command::WriteFileChunk(value) => {
            Some(observer::Command::WriteFileChunk(value.clone()))
        }
        wire::Command::FinishFileWrite(value) => {
            Some(observer::Command::FinishFileWrite(value.clone()))
        }
        wire::Command::CancelFileTransfer(value) => {
            Some(observer::Command::CancelFileTransfer(value.clone()))
        }
        wire::Command::WriteImageFile(value) => {
            Some(observer::Command::WriteImageFile(value.clone()))
        }
        wire::Command::ReadFileAsset(value) => {
            Some(observer::Command::ReadFileAsset(value.clone()))
        }
        wire::Command::GetSessionStats(value) => {
            Some(observer::Command::GetSessionStats(value.clone()))
        }
        wire::Command::ResolveDirectory(value) => {
            Some(observer::Command::ResolveDirectory(value.clone()))
        }
        wire::Command::CreateDirectory(value) => {
            Some(observer::Command::CreateDirectory(value.clone()))
        }
        wire::Command::CompleteDirectory(value) => {
            Some(observer::Command::CompleteDirectory(value.clone()))
        }
        wire::Command::AttachTerminal(_)
        | wire::Command::TakeTerminalControl(_)
        | wire::Command::WriteTerminal(_)
        | wire::Command::ResizeTerminal(_)
        | wire::Command::AcknowledgeTerminal(_)
        | wire::Command::DetachTerminal(_)
        | wire::Command::BeginTerminalUpload(_)
        | wire::Command::WriteTerminalUpload(_)
        | wire::Command::FinishTerminalUpload(_)
        | wire::Command::CancelTerminalUpload(_) => None,
    }
}
pub fn validate_device(actor: &Actor) -> Result<(), AppError> {
    if actor.permission != ViewPermission::Write {
        return Err(error(
            ErrorCode::AccessDenied,
            "Current fixed approved-device authority is required.",
            false,
        ));
    }
    crate::file_ops::capability(actor, "")?;
    Ok(())
}
pub fn authorize(actor: &Actor, command: &wire::Command) -> Result<(), AppError> {
    command
        .validate()
        .map_err(|_| error(ErrorCode::Validation, "Invalid manager-v2 request.", false))?;
    validate_device(actor)?;
    if let Some(compatible) = compatible(command) {
        crate::observer_policy::authorize(actor.permission, &compatible)?;
    }
    Ok(())
}
pub(crate) fn validate_result(
    command: &wire::Command,
    value: &serde_json::Value,
) -> Result<(), AppError> {
    fn response<R: wire::Request>(request: &R, value: &serde_json::Value) -> Result<(), AppError> {
        let value: R::Response = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        value.validate().map_err(|_| invalid())?;
        request.validate_response(&value).map_err(|_| invalid())
    }
    match command {
        wire::Command::GetSnapshot(request) => response(request, value),
        wire::Command::GetTerminalProfiles(request) => response(request, value),
        wire::Command::GetHomeDirectory(request) => response(request, value),
        wire::Command::CreateSession(request) => response(request, value),
        wire::Command::AddTab(request) => response(request, value),
        wire::Command::ActivateSession(request) => response(request, value),
        wire::Command::RenameSession(request) => response(request, value),
        wire::Command::RenameTab(request) => response(request, value),
        wire::Command::ReorderTabs(request) => response(request, value),
        wire::Command::SetSessionPinned(request) => response(request, value),
        wire::Command::SettleSession(request) => response(request, value),
        wire::Command::UnsettleSession(request) => response(request, value),
        wire::Command::DeleteSession(request) => response(request, value),
        wire::Command::ClearSessionError(request) => response(request, value),
        wire::Command::SetSessionEnv(request) => response(request, value),
        wire::Command::SetTerminalEnv(request) => response(request, value),
        wire::Command::SetVisibleSessions(request) => response(request, value),
        wire::Command::CloseTab(request) => response(request, value),
        wire::Command::ListFiles(request) => response(request, value),
        wire::Command::ReadFile(request) => response(request, value),
        wire::Command::WriteFile(request) => response(request, value),
        wire::Command::BeginFileRead(request) => response(request, value),
        wire::Command::ReadFileChunk(request) => response(request, value),
        wire::Command::BeginFileWrite(request) => response(request, value),
        wire::Command::WriteFileChunk(request) => response(request, value),
        wire::Command::FinishFileWrite(request) => response(request, value),
        wire::Command::CancelFileTransfer(request) => response(request, value),
        wire::Command::WriteImageFile(request) => response(request, value),
        wire::Command::ReadFileAsset(request) => response(request, value),
        wire::Command::GetSessionStats(request) => response(request, value),
        wire::Command::ResolveDirectory(request) => response(request, value),
        wire::Command::CreateDirectory(request) => response(request, value),
        wire::Command::CompleteDirectory(request) => response(request, value),
        wire::Command::AttachTerminal(request) => response(request, value),
        wire::Command::TakeTerminalControl(request) => response(request, value),
        wire::Command::WriteTerminal(request) => response(request, value),
        wire::Command::ResizeTerminal(request) => response(request, value),
        wire::Command::AcknowledgeTerminal(request) => response(request, value),
        wire::Command::DetachTerminal(request) => response(request, value),
        wire::Command::BeginTerminalUpload(request) => response(request, value),
        wire::Command::WriteTerminalUpload(request) => response(request, value),
        wire::Command::FinishTerminalUpload(request) => response(request, value),
        wire::Command::CancelTerminalUpload(request) => response(request, value),
    }
}
fn invalid() -> AppError {
    error(
        ErrorCode::Internal,
        "Invalid or uncorrelated manager-v2 owner result.",
        false,
    )
}
