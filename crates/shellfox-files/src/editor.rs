mod stream;
#[cfg(target_os = "linux")]
mod held;
#[cfg(target_os = "linux")]
pub use held::HeldEditor;
#[cfg(target_os = "linux")]
mod wsl_guest;
#[cfg(target_os = "linux")]
pub use wsl_guest::serve_wsl_files;
use crate::*;
use std::sync::{Arc, Mutex};
pub use stream::{EditorFileSink, EditorFileSource};
use uuid::Uuid;

mod cancellation;
pub use cancellation::EditorCancellation;

/// App-owned editor backend bound to a stable, validated private metadata directory.
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
        match self.read_content(input, cancellation, progress, true)? {
            ReadResult::Text {
                path,
                text,
                size,
                revision,
                ..
            } => Ok(TextFile {
                path,
                text,
                size,
                revision,
            }),
            _ => Err(unavailable()),
        }
    }
    pub fn read_document(
        &self,
        input: &Location,
        cancellation: &EditorCancellation,
    ) -> Result<ReadResult> {
        self.read_content(input, cancellation, |_| {}, false)
    }
    fn read_content(
        &self,
        input: &Location,
        cancellation: &EditorCancellation,
        progress: impl Fn(u64),
        text_only: bool,
    ) -> Result<ReadResult> {
        cancellation.check()?;
        let mut registry = self.registry.lock().map_err(|_| unavailable())?;
        let check = |_: &TransferActor, _: &FileService, _: bool| cancellation.check();
        let begin = BeginRead {
            path: input.path.clone(),
            asset_path: None,
        };
        let started = if text_only {
            registry.begin_read_capped(
                &self.actor,
                self.capability.clone(),
                &begin,
                &check,
                TRANSFER_TEXT_BYTES,
            )?
        } else {
            registry.begin_read(&self.actor, self.capability.clone(), &begin, &check)?
        };
        let id = started.transfer_id;
        let result = (|| {
            if text_only && started.kind != ContentKind::Text {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Only UTF-8 text up to 8 MiB can be edited.",
                ));
            }
            let ReadStarted {
                path,
                mime_type,
                size,
                revision,
                kind,
                ..
            } = started;
            if kind == ContentKind::Binary {
                return Ok(ReadResult::Binary {
                    path,
                    mime_type,
                    size,
                    revision,
                });
            }
            let (limit, message) = match kind {
                ContentKind::Text => (
                    TRANSFER_TEXT_BYTES,
                    "Text preview is limited to 8 MiB. Use Download for larger files.",
                ),
                ContentKind::Image => (
                    TRANSFER_IMAGE_BYTES,
                    "Image and asset preview is limited to 64 MiB. Use Download for larger files.",
                ),
                ContentKind::Binary => return Err(unavailable()),
            };
            if size > limit {
                return Err(FileError::new(ErrorCode::Unsupported, message));
            }
            let mut bytes = Vec::with_capacity(usize::try_from(size).map_err(|_| unavailable())?);
            let mut offset = 0;
            loop {
                let chunk = registry.read_chunk(
                    &self.actor,
                    &ReadChunk {
                        transfer_id: id,
                        offset,
                    },
                    &check,
                )?;
                if chunk.data.len() > CHUNK_BYTES
                    || offset + chunk.data.len() as u64 != chunk.offset
                    || chunk.offset > size
                    || chunk.eof != (chunk.offset == size)
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
            if kind == ContentKind::Image {
                return Ok(ReadResult::Image {
                    path,
                    mime_type,
                    size,
                    revision,
                    bytes,
                });
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
            Ok(ReadResult::Text {
                path,
                mime_type,
                size,
                revision,
                text,
            })
        })();
        let _ = registry.cancel(&self.actor, &TransferIdentity { transfer_id: id });
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
        self.write_bytes(
            BeginWrite {
                path: input.path.clone(),
                size: input.text.len() as u64,
                mode: WriteMode::Replace,
                revision: Some(input.revision.clone()),
                content: TransferContent::Text,
                mime_type: None,
            },
            input.text.as_bytes(),
            cancellation,
            progress,
        )
    }

    pub fn write_image(
        &self,
        input: &ImageWrite,
        cancellation: &EditorCancellation,
    ) -> Result<WriteResult> {
        cancellation.check()?;
        self.write_bytes(
            BeginWrite {
                path: input.path.clone(),
                size: input.bytes.len() as u64,
                mode: WriteMode::Replace,
                revision: Some(input.revision.clone()),
                content: TransferContent::Image,
                mime_type: Some(input.mime_type.clone()),
            },
            &input.bytes,
            cancellation,
            |_| {},
        )
    }

    fn write_bytes(
        &self,
        request: BeginWrite,
        bytes: &[u8],
        cancellation: &EditorCancellation,
        progress: impl Fn(u64),
    ) -> Result<WriteResult> {
        let mut registry = self.registry.lock().map_err(|_| unavailable())?;
        let check = |_: &TransferActor, _: &FileService, _: bool| cancellation.check();
        let started =
            registry.begin_write(&self.actor, self.capability.clone(), &request, &check)?;
        let result = (|| {
            let mut offset = 0;
            for bytes in bytes.chunks(CHUNK_BYTES) {
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
