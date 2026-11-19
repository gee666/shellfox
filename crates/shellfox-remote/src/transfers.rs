//! Unchanged v1 transfer DTOs over explicit approved-device OS-account authority.
#[cfg(test)]
#[path = "transfer_tests.rs"]
mod tests;
use crate::{capability_unavailable, error, file_ops, Actor, ViewPermission};
use shellfox_files as files;
use shellfox_protocol::{self as wire, AppError, ErrorCode, RemoteCommand};

pub struct RemoteTransfers {
    registry: files::TransferRegistry,
}
impl RemoteTransfers {
    /// Trusted owner startup only. Never derive the journal directory from a
    /// remote path. The shared registry recovers owned partials before use.
    pub fn open(
        limits: files::TransferLimits,
        private_journal_directory: impl AsRef<std::path::Path>,
    ) -> Result<Self, AppError> {
        let mut registry = files::TransferRegistry::new(limits).map_err(file_ops::file_error)?;
        registry
            .initialize_journal(journal_path(private_journal_directory.as_ref()))
            .map_err(file_ops::file_error)?;
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
        command
            .validate()
            .map_err(|_| error(ErrorCode::Validation, "Invalid transfer request.", false))?;
        let identity = files::TransferActor::new(
            format!("{}/{}", actor.peer_id, actor.client_id),
            wire::ClientId::from_u128(u128::from(actor.view_epoch())),
        )
        .map_err(file_ops::file_error)?;
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
                    return Err(files::FileError {
                        code: files::ErrorCode::AccessDenied,
                        message: "Transfer authorization changed.".into(),
                    });
                }
                // The owner holds live approval through commit; FileService separately
                // checks its revocable capability before each read/write/publication.
                Ok(())
            };
        use wire::RemoteCommand::*;
        match command {
            BeginFileRead(input) => {
                let capability = file_ops::capability(actor, &input.path)?.clone();
                let path = capability
                    .browser_existing(&files::Location {
                        path: input.path.clone(),
                    })
                    .map_err(file_ops::file_error)?
                    .path;
                let result = self
                    .registry
                    .begin_read(
                        &identity,
                        capability,
                        &files::BeginRead {
                            path,
                            asset_path: input.asset_path.clone(),
                        },
                        &auth,
                    )
                    .map_err(file_ops::file_error)?;
                file_ops::checked(wire::FileReadTransfer {
                    transfer_id: result.transfer_id,
                    path: result.path,
                    size: result.size,
                    revision: result.revision,
                    kind: match result.kind {
                        files::ContentKind::Text => wire::FileReadKind::Text,
                        files::ContentKind::Image => wire::FileReadKind::Image,
                        files::ContentKind::Binary => wire::FileReadKind::Binary,
                    },
                    mime_type: result.mime_type,
                    chunk_bytes: result.chunk_bytes,
                })
            }
            ReadFileChunk(input) => {
                let result = self
                    .registry
                    .read_chunk(
                        &identity,
                        &files::ReadChunk {
                            transfer_id: input.transfer_id,
                            offset: input.offset,
                        },
                        &auth,
                    )
                    .map_err(file_ops::file_error)?;
                file_ops::checked(wire::FileChunk {
                    offset: result.offset,
                    data: wire::encode_chunk(&result.data).map_err(|_| {
                        error(ErrorCode::Internal, "Invalid file chunk size.", false)
                    })?,
                    eof: result.eof,
                })
            }
            BeginFileWrite(input) => {
                let capability = file_ops::capability(actor, &input.path)?.clone();
                let result = self
                    .registry
                    .begin_write(
                        &identity,
                        capability,
                        &files::BeginWrite {
                            path: input.path.clone(),
                            size: input.size,
                            mode: match input.mode {
                                wire::FileWriteMode::Create => files::WriteMode::Create,
                                wire::FileWriteMode::Replace => files::WriteMode::Replace,
                            },
                            revision: input.revision.clone(),
                            content: match input.content {
                                wire::FileContent::File => files::TransferContent::File,
                                wire::FileContent::Text => files::TransferContent::Text,
                                wire::FileContent::Image => files::TransferContent::Image,
                            },
                            mime_type: input.mime_type.map(|mime| {
                                match mime {
                                    wire::ImageWriteMime::Png => "image/png",
                                    wire::ImageWriteMime::Jpeg => "image/jpeg",
                                    wire::ImageWriteMime::Webp => "image/webp",
                                }
                                .into()
                            }),
                        },
                        &auth,
                    )
                    .map_err(file_ops::file_error)?;
                file_ops::checked(wire::FileWriteTransfer {
                    transfer_id: result.transfer_id,
                    chunk_bytes: result.chunk_bytes,
                })
            }
            WriteFileChunk(input) => {
                let data = wire::decode_chunk(&input.data).map_err(|_| {
                    error(ErrorCode::Validation, "Invalid file chunk encoding.", false)
                })?;
                let result = self
                    .registry
                    .write_chunk(
                        &identity,
                        &files::WriteChunk {
                            transfer_id: input.transfer_id,
                            offset: input.offset,
                            data,
                        },
                        &auth,
                    )
                    .map_err(file_ops::file_error)?;
                file_ops::checked(wire::TransferOffset {
                    offset: result.offset,
                })
            }
            FinishFileWrite(input) => {
                let result = self
                    .registry
                    .finish_write(
                        &identity,
                        &files::TransferIdentity {
                            transfer_id: input.transfer_id,
                        },
                        &auth,
                    )
                    .map_err(file_ops::file_error)?;
                file_ops::checked(wire::FileTransferResult {
                    path: result.path,
                    size: result.size,
                    revision: result.revision,
                })
            }
            CancelFileTransfer(input) => {
                self.registry
                    .cancel(
                        &identity,
                        &files::TransferIdentity {
                            transfer_id: input.transfer_id,
                        },
                    )
                    .map_err(file_ops::file_error)?;
                file_ops::checked(wire::Cancelled { cancelled: true })
            }
            _ => Err(capability_unavailable("not a file transfer command")),
        }
    }
}
use wire::Validate;
fn journal_path(path: &std::path::Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut parts = path.components();
        if let Some(Component::Prefix(prefix)) = parts.next() {
            if let Prefix::VerbatimDisk(drive) = prefix.kind() {
                if parts.next() == Some(Component::RootDir) {
                    let mut ordinary =
                        std::path::PathBuf::from(format!("{}:\\", char::from(drive)));
                    ordinary.push(parts.as_path());
                    return ordinary;
                }
            }
        }
    }
    path.to_owned()
}
