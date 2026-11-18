use super::{ClientError, ObserverClient};
use shellfox_protocol::{observer_v2::*, OutboundTransport};
impl<T: OutboundTransport> ObserverClient<T> {
    /// One typed 43-verb mapping; presentation JSON is created only AFTER response validation.
    pub async fn execute_command(
        &self,
        command: &Command,
    ) -> Result<Reply<serde_json::Value>, ClientError> {
        match command {
            Command::GetSnapshot(request) => self.erase(request).await,
            Command::GetTerminalProfiles(request) => self.erase(request).await,
            Command::GetHomeDirectory(request) => self.erase(request).await,
            Command::CreateSession(request) => self.erase(request).await,
            Command::AddTab(request) => self.erase(request).await,
            Command::ActivateSession(request) => self.erase(request).await,
            Command::RenameSession(request) => self.erase(request).await,
            Command::RenameTab(request) => self.erase(request).await,
            Command::ReorderTabs(request) => self.erase(request).await,
            Command::SetSessionPinned(request) => self.erase(request).await,
            Command::SettleSession(request) => self.erase(request).await,
            Command::UnsettleSession(request) => self.erase(request).await,
            Command::DeleteSession(request) => self.erase(request).await,
            Command::ClearSessionError(request) => self.erase(request).await,
            Command::SetSessionEnv(request) => self.erase(request).await,
            Command::SetTerminalEnv(request) => self.erase(request).await,
            Command::SetVisibleSessions(request) => self.erase(request).await,
            Command::AttachTerminal(request) => self.erase(request).await,
            Command::TakeTerminalControl(request) => self.erase(request).await,
            Command::WriteTerminal(request) => self.erase(request).await,
            Command::ResizeTerminal(request) => self.erase(request).await,
            Command::AcknowledgeTerminal(request) => self.erase(request).await,
            Command::DetachTerminal(request) => self.erase(request).await,
            Command::CloseTab(request) => self.erase(request).await,
            Command::ListFiles(request) => self.erase(request).await,
            Command::ReadFile(request) => self.erase(request).await,
            Command::WriteFile(request) => self.erase(request).await,
            Command::BeginFileRead(request) => self.erase(request).await,
            Command::ReadFileChunk(request) => self.erase(request).await,
            Command::BeginFileWrite(request) => self.erase(request).await,
            Command::WriteFileChunk(request) => self.erase(request).await,
            Command::FinishFileWrite(request) => self.erase(request).await,
            Command::CancelFileTransfer(request) => self.erase(request).await,
            Command::WriteImageFile(request) => self.erase(request).await,
            Command::ReadFileAsset(request) => self.erase(request).await,
            Command::GetSessionStats(request) => self.erase(request).await,
            Command::ResolveDirectory(request) => self.erase(request).await,
            Command::CreateDirectory(request) => self.erase(request).await,
            Command::CompleteDirectory(request) => self.erase(request).await,
            Command::BeginTerminalUpload(request) => self.erase(request).await,
            Command::WriteTerminalUpload(request) => self.erase(request).await,
            Command::FinishTerminalUpload(request) => self.erase(request).await,
            Command::CancelTerminalUpload(request) => self.erase(request).await,
        }
    }
    async fn erase<R: Request>(
        &self,
        request: &R,
    ) -> Result<Reply<serde_json::Value>, ClientError> {
        match self.execute(request).await? {
            Reply::Success(value) => Ok(Reply::Success(
                serde_json::to_value(value).map_err(|_| ClientError::InvalidResponse)?,
            )),
            Reply::Failure(error) => Ok(Reply::Failure(error)),
        }
    }
}
