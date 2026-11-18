use super::{NativeAction, NativeClient, NativeResponse};
use shellfox_core::SessionId;
use shellfox_host::CreationRequestReceipt;

impl NativeClient {
    /// Query only after CreateSnapshot success, using that originating client.
    /// Callers must retain their original navigation epoch. Unavailable never
    /// identifies a replacement session and must not cause focus or recreation.
    pub async fn creation_request_receipt(
        &self,
        request_id: SessionId,
    ) -> anyhow::Result<CreationRequestReceipt> {
        match self
            .execute(NativeAction::CreationRequestReceipt { request_id })
            .await?
        {
            NativeResponse::CreationRequestReceipt {
                request_id: actual,
                receipt,
            } if actual == request_id => Ok(receipt),
            _ => anyhow::bail!("Host creation receipt reply does not match its request."),
        }
    }
}
