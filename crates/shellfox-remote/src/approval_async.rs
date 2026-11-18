//! Policy locks serialize owner commits. Never wait on them on a Tokio worker.
use crate::{approvals::*, error};
use shellfox_protocol::{AppError, ClientId, ErrorCode};
use shellfox_transport::incoming::PeerIdentity;
pub(crate) async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| error(ErrorCode::Internal, "Approval worker stopped.", false))?
}
impl ApprovalControl {
    pub async fn state_async(&self) -> Result<ApprovalState, AppError> {
        let control = self.clone();
        blocking(move || control.state()).await
    }
    pub async fn set_enabled_async(&self, enabled: bool) -> Result<(), AppError> {
        let control = self.clone();
        blocking(move || control.set_enabled(enabled)).await
    }
    pub async fn reject_async(&self, peer: String) -> Result<(), AppError> {
        let control = self.clone();
        blocking(move || control.reject(&peer)).await
    }
    pub async fn authorize_async(
        &self,
        peer: PeerIdentity,
        client: ClientId,
        now: u64,
    ) -> Result<Actor, AppError> {
        let control = self.clone();
        blocking(move || control.authorize(&peer, client, now)).await
    }
    pub async fn withdraw_async(
        &self,
        peer: PeerIdentity,
        client: ClientId,
        now: u64,
    ) -> Result<(), AppError> {
        let control = self.clone();
        blocking(move || control.withdraw(&peer, client, now)).await
    }
    pub async fn expire_pending_async(&self, now: u64) -> Result<(), AppError> {
        let control = self.clone();
        blocking(move || {
            control.expire_pending(now);
            Ok(())
        })
        .await
    }
}
impl Actor {
    pub async fn validate_async(&self) -> Result<(), AppError> {
        let actor = self.clone();
        blocking(move || actor.with_authorized(None, || Ok(()))).await
    }
}
