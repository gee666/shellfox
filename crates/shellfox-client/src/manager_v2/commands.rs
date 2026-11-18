use super::{ClientError, ManagerClient};
use shellfox_protocol::{manager_v2::*, OutboundTransport};
impl<T: OutboundTransport> ManagerClient<T> {
    pub async fn execute_command(
        &self,
        command: &Command,
    ) -> Result<Reply<serde_json::Value>, ClientError> {
        match command {
            Command::GetSnapshot(value) => self.erase(value).await,
            Command::GetTerminalProfiles(value) => self.erase(value).await,
            Command::GetHomeDirectory(value) => self.erase(value).await,
            Command::CreateSession(value) => self.erase(value).await,
            Command::AddTab(value) => self.erase(value).await,
            Command::ActivateSession(value) => self.erase(value).await,
            Command::RenameSession(value) => self.erase(value).await,
            Command::RenameTab(value) => self.erase(value).await,
            Command::ReorderTabs(value) => self.erase(value).await,
            Command::SetSessionPinned(value) => self.erase(value).await,
            Command::SettleSession(value) => self.erase(value).await,
            Command::UnsettleSession(value) => self.erase(value).await,
            Command::DeleteSession(value) => self.erase(value).await,
            Command::ClearSessionError(value) => self.erase(value).await,
            Command::SetSessionEnv(value) => self.erase(value).await,
            Command::SetTerminalEnv(value) => self.erase(value).await,
            Command::SetVisibleSessions(value) => self.erase(value).await,
            Command::AttachTerminal(value) => self.erase(value).await,
            Command::TakeTerminalControl(value) => self.erase(value).await,
            Command::WriteTerminal(value) => self.erase(value).await,
            Command::ResizeTerminal(value) => self.erase(value).await,
            Command::AcknowledgeTerminal(value) => self.erase(value).await,
            Command::DetachTerminal(value) => self.erase(value).await,
            Command::CloseTab(value) => self.erase(value).await,
            Command::ListFiles(value) => self.erase(value).await,
            Command::ReadFile(value) => self.erase(value).await,
            Command::WriteFile(value) => self.erase(value).await,
            Command::BeginFileRead(value) => self.erase(value).await,
            Command::ReadFileChunk(value) => self.erase(value).await,
            Command::BeginFileWrite(value) => self.erase(value).await,
            Command::WriteFileChunk(value) => self.erase(value).await,
            Command::FinishFileWrite(value) => self.erase(value).await,
            Command::CancelFileTransfer(value) => self.erase(value).await,
            Command::WriteImageFile(value) => self.erase(value).await,
            Command::ReadFileAsset(value) => self.erase(value).await,
            Command::GetSessionStats(value) => self.erase(value).await,
            Command::ResolveDirectory(value) => self.erase(value).await,
            Command::CreateDirectory(value) => self.erase(value).await,
            Command::CompleteDirectory(value) => self.erase(value).await,
            Command::BeginTerminalUpload(value) => self.erase(value).await,
            Command::WriteTerminalUpload(value) => self.erase(value).await,
            Command::FinishTerminalUpload(value) => self.erase(value).await,
            Command::CancelTerminalUpload(value) => self.erase(value).await,
        }
    }
    async fn erase<R: Request>(
        &self,
        request: &R,
    ) -> Result<Reply<serde_json::Value>, ClientError> {
        match self.execute(request).await? {
            Reply::Success(v) => Ok(Reply::Success(
                serde_json::to_value(v).map_err(|_| ClientError::InvalidResponse)?,
            )),
            Reply::Failure(e) => Ok(Reply::Failure(e)),
        }
    }
}
