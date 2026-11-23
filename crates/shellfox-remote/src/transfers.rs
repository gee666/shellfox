//! Unchanged v1 transfer DTOs over explicit approved-device OS-account authority.
#[cfg(test)]
#[path = "transfer_tests.rs"]
mod tests;
use crate::{file_ops, transfer_bridge, Actor, ViewPermission};
use shellfox_files as files;
use shellfox_protocol::{self as wire, AppError, RemoteCommand};

pub struct RemoteTransfers {
    registry: files::TransferRegistry,
}
impl RemoteTransfers {
    pub fn open(limits: files::TransferLimits) -> Result<Self, AppError> {
        let registry = files::TransferRegistry::new(limits).map_err(file_ops::file_error)?;
        Ok(Self { registry })
    }
    pub fn cancel_actor(&mut self, owner: &str) {
        self.registry.cancel_actor(owner);
    }
    /// Call inside the owner's Actor::with_authorized commit guard.
    pub fn request(
        &mut self,
        actor: &Actor,
        command: &RemoteCommand,
    ) -> Result<serde_json::Value, AppError> {
        let transfer = transfer_bridge::decode(command)?;
        let capability = match &transfer {
            files::TransferRequest::BeginRead(input) => {
                Some(file_ops::capability(actor, &input.path)?.clone())
            }
            files::TransferRequest::BeginWrite(input) => {
                Some(file_ops::capability(actor, &input.path)?.clone())
            }
            _ => None,
        };
        let identity = files::TransferActor::new(
            format!("{}/{}", actor.peer_id, actor.client_id),
            wire::ClientId::from_u128(u128::from(actor.view_epoch())),
        )
        .map_err(file_ops::file_error)?;
        let denied = || files::FileError {
            code: files::ErrorCode::AccessDenied,
            message: "Transfer authorization changed.".into(),
        };
        let auth =
            |requester: &files::TransferActor, capability: &files::FileService, write: bool| {
                if requester != &identity
                    || (write && actor.permission != ViewPermission::Write)
                    || !actor
                        .file_capabilities()
                        .iter()
                        .any(|cap| std::ptr::eq(cap.as_ref(), capability))
                    || capability.capability_kind() != files::CapabilityKind::ApprovedFullAccount
                {
                    return Err(denied());
                }
                Ok(())
            };
        let reply = self
            .registry
            .execute(
                &identity,
                transfer,
                |_| capability.ok_or_else(denied),
                &auth,
            )
            .map_err(file_ops::file_error)?;
        transfer_bridge::encode(reply)
    }
}
