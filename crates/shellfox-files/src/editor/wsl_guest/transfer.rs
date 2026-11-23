use super::*;
use crate::{TransferAuthorization, CHUNK_BYTES};

impl GuestEditor {
    pub(super) fn transfer(
        &mut self,
        mut request: TransferRequest,
        bytes: Vec<u8>,
    ) -> Result<(Reply, Vec<u8>)> {
        if let TransferRequest::Write(input) = &mut request {
            if !input.data.is_empty() || bytes.len() > CHUNK_BYTES {
                return Err(bad_payload());
            }
            input.data = bytes;
        }
        let mut registry = self.backend.registry.lock().map_err(|_| unavailable())?;
        // The Windows owner authorizes each message before writing to this private
        // pipe. The guest registry never accepts another connection or capability.
        let auth = |actor: &TransferActor, service: &FileService, _| {
            if actor == &self.transfer_actor && std::ptr::eq(service, self.service.as_ref()) {
                Ok(())
            } else {
                Err(FileError::denied())
            }
        };
        let auth: &dyn TransferAuthorization = &auth;
        let mut reply = registry.execute(
            &self.transfer_actor,
            request,
            |_| Ok(self.service.clone()),
            auth,
        )?;
        let bytes = match &mut reply {
            TransferReply::Chunk(chunk) => std::mem::take(&mut chunk.data),
            _ => Vec::new(),
        };
        Ok((Reply::Transfer(reply), bytes))
    }
}
