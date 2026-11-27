use super::WslBackend;
use anyhow::{Result, bail, ensure};
use shellfox_files::{
    BeginWrite, TransferContent, TransferIdentity, TransferReply, TransferRequest, WriteMode,
    wsl_files_ipc::{Reply, Request},
};

impl WslBackend {
    /// An empty create-mode transfer on one worker, so the guest publishes without replacing.
    pub(in crate::files::backend) fn create_file(&self, path: &str) -> Result<String> {
        let path = self.location(path)?.path;
        let worker = self.start()?;
        let call = |request| {
            Self::without_payload(worker.exchange(Request::Transfer(request), Vec::new())?)
        };
        let Reply::Transfer(TransferReply::WriteStarted(started)) =
            call(TransferRequest::BeginWrite(BeginWrite {
                path: path.clone(),
                size: 0,
                mode: WriteMode::Create,
                revision: None,
                content: TransferContent::Text,
                mime_type: None,
            }))?
        else {
            bail!("Unexpected WSL transfer response")
        };
        let Reply::Transfer(TransferReply::Finished(result)) =
            call(TransferRequest::Finish(TransferIdentity {
                transfer_id: started.transfer_id,
            }))?
        else {
            bail!("WSL create was not confirmed. Refresh the folder.")
        };
        ensure!(
            result.path == path && result.size == 0,
            "WSL create was not confirmed. Refresh the folder."
        );
        self.check()?;
        Ok(result.path)
    }
}
