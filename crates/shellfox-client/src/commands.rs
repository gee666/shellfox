use crate::{Client, ClientError};
use shellfox_protocol::*;

impl<T: OutboundTransport> Client<T> {
    /// Erases only a validated typed response for presentation bindings.
    /// Callers cannot select arbitrary methods or bypass the remote allowlist.
    pub async fn execute_command(
        &self,
        command: &RemoteCommand,
    ) -> Result<Reply<serde_json::Value>, ClientError> {
        match command {
            RemoteCommand::GetSnapshot(request) => self.command_response(request).await,
            RemoteCommand::GetTerminalProfiles(request) => self.command_response(request).await,
            RemoteCommand::GetHomeDirectory(request) => self.command_response(request).await,
            RemoteCommand::CreateSession(request) => self.command_response(request).await,
            RemoteCommand::AddTab(request) => self.command_response(request).await,
            RemoteCommand::ActivateSession(request) => self.command_response(request).await,
            RemoteCommand::RenameSession(request) => self.command_response(request).await,
            RemoteCommand::RenameTab(request) => self.command_response(request).await,
            RemoteCommand::ReorderTabs(request) => self.command_response(request).await,
            RemoteCommand::SetSessionPinned(request) => self.command_response(request).await,
            RemoteCommand::SettleSession(request) => self.command_response(request).await,
            RemoteCommand::UnsettleSession(request) => self.command_response(request).await,
            RemoteCommand::DeleteSession(request) => self.command_response(request).await,
            RemoteCommand::ClearSessionError(request) => self.command_response(request).await,
            RemoteCommand::SetSessionEnv(request) => self.command_response(request).await,
            RemoteCommand::SetTerminalEnv(request) => self.command_response(request).await,
            RemoteCommand::SetVisibleSessions(request) => self.command_response(request).await,
            RemoteCommand::AttachTerminal(request) => self.command_response(request).await,
            RemoteCommand::TakeTerminalControl(request) => self.command_response(request).await,
            RemoteCommand::WriteTerminal(request) => self.command_response(request).await,
            RemoteCommand::ResizeTerminal(request) => self.command_response(request).await,
            RemoteCommand::AcknowledgeTerminal(request) => self.command_response(request).await,
            RemoteCommand::DetachTerminal(request) => self.command_response(request).await,
            RemoteCommand::CloseTab(request) => self.command_response(request).await,
            RemoteCommand::ListFiles(request) => self.command_response(request).await,
            RemoteCommand::ReadFile(request) => self.command_response(request).await,
            RemoteCommand::WriteFile(request) => self.command_response(request).await,
            RemoteCommand::BeginFileRead(request) => self.command_response(request).await,
            RemoteCommand::ReadFileChunk(request) => self.command_response(request).await,
            RemoteCommand::BeginFileWrite(request) => self.command_response(request).await,
            RemoteCommand::WriteFileChunk(request) => self.command_response(request).await,
            RemoteCommand::FinishFileWrite(request) => self.command_response(request).await,
            RemoteCommand::CancelFileTransfer(request) => self.command_response(request).await,
            RemoteCommand::WriteImageFile(request) => self.command_response(request).await,
            RemoteCommand::ReadFileAsset(request) => self.command_response(request).await,
            RemoteCommand::GetSessionStats(request) => self.command_response(request).await,
            RemoteCommand::ResolveDirectory(request) => self.command_response(request).await,
            RemoteCommand::CreateDirectory(request) => self.command_response(request).await,
            RemoteCommand::CompleteDirectory(request) => self.command_response(request).await,
            RemoteCommand::BeginTerminalUpload(request) => self.command_response(request).await,
            RemoteCommand::WriteTerminalUpload(request) => self.command_response(request).await,
            RemoteCommand::FinishTerminalUpload(request) => self.command_response(request).await,
            RemoteCommand::CancelTerminalUpload(request) => self.command_response(request).await,
        }
    }
    async fn command_response<R: Request>(
        &self,
        request: &R,
    ) -> Result<Reply<serde_json::Value>, ClientError> {
        match self.execute(request).await? {
            Reply::Success(value) => Ok(Reply::Success(
                serde_json::to_value(value).map_err(|_| ValidationError)?,
            )),
            Reply::Failure(error) => Ok(Reply::Failure(error)),
        }
    }
}
