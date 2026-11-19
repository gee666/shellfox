use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "operation", content = "input", deny_unknown_fields)]
pub enum TransferRequest {
    BeginRead(BeginRead),
    BeginWrite(BeginWrite),
    Read(ReadChunk),
    Write(WriteChunk),
    Finish(TransferIdentity),
    Cancel(TransferIdentity),
}

impl TransferRequest {
    pub fn id(&self) -> Option<Uuid> {
        match self {
            Self::BeginRead(_) | Self::BeginWrite(_) => None,
            Self::Read(input) => Some(input.transfer_id),
            Self::Write(input) => Some(input.transfer_id),
            Self::Finish(input) | Self::Cancel(input) => Some(input.transfer_id),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "operation", content = "output", deny_unknown_fields)]
pub enum TransferReply {
    ReadStarted(ReadStarted),
    WriteStarted(WriteStarted),
    Chunk(ReadChunkResult),
    Progress(TransferProgress),
    Finished(WriteResult),
    Cancelled,
}

impl TransferRegistry {
    pub fn execute(
        &mut self,
        actor: &TransferActor,
        request: TransferRequest,
        capability: impl FnOnce(&str) -> Result<Arc<FileService>>,
        auth: &dyn TransferAuthorization,
    ) -> Result<TransferReply> {
        Ok(match request {
            TransferRequest::BeginRead(mut input) => {
                let service = capability(&input.path)?;
                input.path = service.browser_existing(&Location { path: input.path })?.path;
                TransferReply::ReadStarted(self.begin_read(actor, service, &input, auth)?)
            }
            TransferRequest::BeginWrite(input) => {
                let service = capability(&input.path)?;
                TransferReply::WriteStarted(self.begin_write(actor, service, &input, auth)?)
            }
            TransferRequest::Read(input) => TransferReply::Chunk(self.read_chunk(actor, &input, auth)?),
            TransferRequest::Write(input) => TransferReply::Progress(self.write_chunk(actor, &input, auth)?),
            TransferRequest::Finish(input) => TransferReply::Finished(self.finish_write(actor, &input, auth)?),
            TransferRequest::Cancel(input) => {
                self.cancel(actor, &input)?;
                TransferReply::Cancelled
            }
        })
    }
}
