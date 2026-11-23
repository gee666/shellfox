use super::{unavailable, EditorCancellation, EditorSession};
use crate::*;
use std::{
    io::{self, Cursor, Read},
    sync::Arc,
};
use uuid::Uuid;
#[cfg(test)]
mod tests;

struct Lease {
    session: Arc<EditorSession>,
    cancel: EditorCancellation,
    id: Uuid,
    offset: u64,
}
impl Lease {
    fn guard(&self, _: &TransferActor, _: &FileService, _: bool) -> Result<()> {
        self.cancel.check()
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.session.registry.lock() {
            let _ = registry.cancel(
                &self.session.actor,
                &TransferIdentity {
                    transfer_id: self.id,
                },
            );
        }
    }
}

pub struct EditorFileSource {
    lease: Lease,
    header: ReadStarted,
    buffered: Cursor<Vec<u8>>,
}
impl EditorFileSource {
    pub fn header(&self) -> &ReadStarted {
        &self.header
    }
}
impl Read for EditorFileSource {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.lease.cancel.check().map_err(io::Error::other)?;
        if output.is_empty() {
            return Ok(0);
        }
        if self.buffered.position() >= self.buffered.get_ref().len() as u64 {
            if self.lease.offset == self.header.size {
                return Ok(0);
            }
            let mut registry = self
                .lease
                .session
                .registry
                .lock()
                .map_err(|_| io::Error::other(unavailable()))?;
            let chunk = registry
                .read_chunk(
                    &self.lease.session.actor,
                    &ReadChunk {
                        transfer_id: self.lease.id,
                        offset: self.lease.offset,
                    },
                    &|actor: &TransferActor, service: &FileService, write| {
                        self.lease.guard(actor, service, write)
                    },
                )
                .map_err(io::Error::other)?;
            self.lease.offset = chunk.offset;
            self.buffered = Cursor::new(chunk.data);
        }
        self.buffered.read(output)
    }
}

pub struct EditorFileSink(Lease);
impl EditorFileSink {
    pub fn append(&mut self, bytes: &[u8]) -> Result<()> {
        let mut registry = self.0.session.registry.lock().map_err(|_| unavailable())?;
        let next = registry.write_chunk(
            &self.0.session.actor,
            &WriteChunk {
                transfer_id: self.0.id,
                offset: self.0.offset,
                data: bytes.to_vec(),
            },
            &|actor: &TransferActor, service: &FileService, write| {
                self.0.guard(actor, service, write)
            },
        )?;
        self.0.offset = next.offset;
        Ok(())
    }

    pub fn finish(self) -> Result<WriteResult> {
        let mut registry = self.0.session.registry.lock().map_err(|_| unavailable())?;
        registry.finish_write(
            &self.0.session.actor,
            &TransferIdentity {
                transfer_id: self.0.id,
            },
            &|actor: &TransferActor, service: &FileService, write| {
                self.0.guard(actor, service, write)
            },
        )
    }
}

impl EditorSession {
    pub fn read_asset(
        self: &Arc<Self>,
        input: &AssetRequest,
        cancel: EditorCancellation,
    ) -> Result<FileAsset> {
        let mut source = self.open_source(
            BeginRead {
                path: input.path.clone(),
                asset_path: Some(input.asset_path.clone()),
            },
            cancel,
        )?;
        let header = source.header().clone();
        if header.size > TRANSFER_IMAGE_BYTES {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Image and asset preview is limited to 64 MiB. Use Download for larger files.",
            ));
        }
        let mut bytes = Vec::with_capacity(header.size as usize);
        source.read_to_end(&mut bytes)?;
        if bytes.len() as u64 != header.size {
            return Err(unavailable());
        }
        Ok(FileAsset {
            mime_type: header.mime_type,
            size: header.size,
            bytes,
        })
    }

    pub fn file_source(
        self: &Arc<Self>,
        path: &str,
        cancel: EditorCancellation,
    ) -> Result<EditorFileSource> {
        self.open_source(
            BeginRead {
                path: path.into(),
                asset_path: None,
            },
            cancel,
        )
    }

    fn open_source(
        self: &Arc<Self>,
        request: BeginRead,
        cancel: EditorCancellation,
    ) -> Result<EditorFileSource> {
        let mut registry = self.registry.lock().map_err(|_| unavailable())?;
        let header = registry.begin_read(
            &self.actor,
            self.capability.clone(),
            &request,
            &|_: &TransferActor, _: &FileService, _: bool| cancel.check(),
        )?;
        Ok(EditorFileSource {
            lease: Lease {
                session: self.clone(),
                cancel,
                id: header.transfer_id,
                offset: 0,
            },
            header,
            buffered: Cursor::default(),
        })
    }

    pub fn file_sink(
        self: &Arc<Self>,
        path: &str,
        size: u64,
        cancel: EditorCancellation,
    ) -> Result<EditorFileSink> {
        cancel.check()?;
        let revision = match self.file_source(path, cancel.clone()) {
            Ok(current) => Some(current.header().revision.clone()),
            Err(error) if error.code == ErrorCode::NotFound => None,
            Err(error) => return Err(error),
        };
        let mode = if revision.is_some() {
            WriteMode::Replace
        } else {
            WriteMode::Create
        };
        let mut registry = self.registry.lock().map_err(|_| unavailable())?;
        let writer = registry.begin_write(
            &self.actor,
            self.capability.clone(),
            &BeginWrite {
                path: path.into(),
                size,
                mode,
                revision,
                content: TransferContent::File,
                mime_type: None,
            },
            &|_: &TransferActor, _: &FileService, _: bool| cancel.check(),
        )?;
        Ok(EditorFileSink(Lease {
            session: self.clone(),
            cancel,
            id: writer.transfer_id,
            offset: 0,
        }))
    }
}
