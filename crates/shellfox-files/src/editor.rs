//! In-process editor adapter over the same capability and journaled chunk contracts.
//! It does not raise inline file/IPC limits or grant filesystem authority.
use crate::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct EditorCancellation(Arc<AtomicBool>);
impl EditorCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(FileError::new(
                ErrorCode::AccessDenied,
                "File operation cancelled.",
            ))
        } else {
            Ok(())
        }
    }
}

/// One app-owned backend. The owner supplies a stable, validated private metadata namespace.
/// Never initialize this from a view, per-run directory, or untrusted IPC path.
pub struct EditorBackend {
    capability: Arc<FileService>,
    registry: Arc<Mutex<TransferRegistry>>,
}
impl EditorBackend {
    pub fn new(
        capability: Arc<FileService>,
        private_journal_directory: impl AsRef<std::path::Path>,
    ) -> Result<Self> {
        capability.require_path_resolution()?;
        let mut registry = TransferRegistry::new(TransferLimits::default())?;
        registry.initialize_journal(private_journal_directory)?;
        Ok(Self {
            capability,
            registry: Arc::new(Mutex::new(registry)),
        })
    }
    /// Retain the exact ApprovedRoot RW journal authority returned by a directory plan.
    pub fn new_anchored(capability: Arc<FileService>, journal: Arc<FileService>) -> Result<Self> {
        capability.require_path_resolution()?;
        let mut registry = TransferRegistry::new(TransferLimits::default())?;
        registry.initialize_journal_anchored(journal)?;
        Ok(Self {
            capability,
            registry: Arc::new(Mutex::new(registry)),
        })
    }
    pub fn capability(&self) -> Arc<FileService> {
        self.capability.clone()
    }
    pub fn session(&self) -> Arc<EditorSession> {
        Arc::new(EditorSession::new(
            self.capability.clone(),
            self.registry.clone(),
        ))
    }
}

pub struct EditorSession {
    capability: Arc<FileService>,
    registry: Arc<Mutex<TransferRegistry>>,
    actor: TransferActor,
}
impl EditorSession {
    /// The caller initializes and shares the private host registry before constructing sessions.
    pub fn new(capability: Arc<FileService>, registry: Arc<Mutex<TransferRegistry>>) -> Self {
        Self {
            capability,
            registry,
            actor: TransferActor {
                owner: format!("editor-{}", Uuid::new_v4()),
                generation: Uuid::new_v4(),
            },
        }
    }
    pub fn read(&self, input: &Location, cancellation: &EditorCancellation) -> Result<TextFile> {
        self.read_with(input, cancellation, |_| {})
    }
    fn read_with(
        &self,
        input: &Location,
        cancellation: &EditorCancellation,
        progress: impl Fn(u64),
    ) -> Result<TextFile> {
        cancellation.check()?;
        let mut registry = self.registry.lock().map_err(|_| unavailable())?;
        let check = |_: &TransferActor, _: &FileService, _: bool| cancellation.check();
        let started = registry.begin_read_capped(
            &self.actor,
            self.capability.clone(),
            &BeginRead {
                path: input.path.clone(),
                asset_path: None,
            },
            &check,
            TRANSFER_TEXT_BYTES,
        )?;
        let result = (|| {
            if started.kind != ContentKind::Text || started.size > TRANSFER_TEXT_BYTES {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Only UTF-8 text up to 8 MiB can be edited.",
                ));
            }
            let mut bytes =
                Vec::with_capacity(usize::try_from(started.size).map_err(|_| unavailable())?);
            let mut offset = 0;
            loop {
                let chunk = registry.read_chunk(
                    &self.actor,
                    &ReadChunk {
                        transfer_id: started.transfer_id,
                        offset,
                    },
                    &check,
                )?;
                if chunk.data.len() > CHUNK_BYTES
                    || offset + chunk.data.len() as u64 != chunk.offset
                    || chunk.offset > started.size
                    || chunk.eof != (chunk.offset == started.size)
                    || chunk.data.is_empty() && !chunk.eof
                {
                    return Err(unavailable());
                }
                bytes.extend_from_slice(&chunk.data);
                offset = chunk.offset;
                progress(offset);
                cancellation.check()?;
                if chunk.eof {
                    break;
                }
            }
            let text = String::from_utf8(bytes).map_err(|_| {
                FileError::new(ErrorCode::Unsupported, "This file is not UTF-8 text.")
            })?;
            if text.contains('\0') {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Editor text cannot contain NUL.",
                ));
            }
            Ok(TextFile {
                path: started.path,
                text,
                size: started.size,
                revision: started.revision,
            })
        })();
        let _ = registry.cancel(
            &self.actor,
            &TransferIdentity {
                transfer_id: started.transfer_id,
            },
        );
        result
    }
    pub fn write(
        &self,
        input: &WriteText,
        cancellation: &EditorCancellation,
    ) -> Result<WriteResult> {
        self.write_with(input, cancellation, |_| {})
    }
    fn write_with(
        &self,
        input: &WriteText,
        cancellation: &EditorCancellation,
        progress: impl Fn(u64),
    ) -> Result<WriteResult> {
        cancellation.check()?;
        if input.text.len() as u64 > TRANSFER_TEXT_BYTES || input.text.contains('\0') {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Text exceeds the 8 MiB editor limit or contains NUL.",
            ));
        }
        let mut registry = self.registry.lock().map_err(|_| unavailable())?;
        let check = |_: &TransferActor, _: &FileService, _: bool| cancellation.check();
        let started = registry.begin_write(
            &self.actor,
            self.capability.clone(),
            &BeginWrite {
                path: input.path.clone(),
                size: input.text.len() as u64,
                mode: WriteMode::Replace,
                revision: Some(input.revision.clone()),
                content: TransferContent::Text,
                mime_type: None,
            },
            &check,
        )?;
        let result = (|| {
            let mut offset = 0;
            for bytes in input.text.as_bytes().chunks(CHUNK_BYTES) {
                let next = registry.write_chunk(
                    &self.actor,
                    &WriteChunk {
                        transfer_id: started.transfer_id,
                        offset,
                        data: bytes.to_vec(),
                    },
                    &check,
                )?;
                if next.offset != offset + bytes.len() as u64 {
                    return Err(unavailable());
                }
                offset = next.offset;
                progress(offset);
                cancellation.check()?;
            }
            registry.finish_write(
                &self.actor,
                &TransferIdentity {
                    transfer_id: started.transfer_id,
                },
                &check,
            )
        })();
        let _ = registry.cancel(
            &self.actor,
            &TransferIdentity {
                transfer_id: started.transfer_id,
            },
        );
        result
    }
}
fn unavailable() -> FileError {
    FileError::new(
        ErrorCode::StorageFailed,
        "The editor transfer returned invalid state. No save was retried.",
    )
}

#[cfg(test)]
mod tests;
