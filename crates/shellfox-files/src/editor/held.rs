//! An open Linux document retains the exact file and parent used for the read.
use super::*;
use crate::{content, paths, staged_write::StagedWrite, stream_io};
use cap_std::fs::File;

pub struct HeldEditor {
    target: paths::Target,
    file: File,
    snapshot: content::Snapshot,
    revision: String,
}

impl HeldEditor {
    pub fn open(service: &FileService, location: &Location) -> Result<(Self, ReadResult)> {
        let location = service.browser_existing(location)?;
        let target = service.target(&location.path)?;
        let mut file = content::open_regular(&target.parent, &target.name)?;
        let inspected =
            stream_io::inspect(&mut file, &target.absolute, TRANSFER_MAX_BYTES, &|| Ok(()))?;
        let stream_io::Inspected {
            size,
            revision,
            kind,
            mime_type,
            snapshot,
        } = inspected;
        let path = location.path;
        let limit = match kind {
            ContentKind::Text => TRANSFER_TEXT_BYTES,
            ContentKind::Image => TRANSFER_IMAGE_BYTES,
            ContentKind::Binary => 0,
        };
        let document = if kind == ContentKind::Binary {
            ReadResult::Binary {
                path,
                mime_type,
                size,
                revision: revision.clone(),
            }
        } else {
            if size > limit {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    if kind == ContentKind::Text {
                        "Text preview is limited to 8 MiB."
                    } else {
                        "Image preview is limited to 64 MiB."
                    },
                ));
            }
            let mut bytes = Vec::with_capacity(size as usize);
            while (bytes.len() as u64) < size {
                bytes.extend(stream_io::checked_read(
                    &mut file,
                    &snapshot,
                    bytes.len() as u64,
                    &|| Ok(()),
                )?);
            }
            if content::revision(&bytes) != revision {
                return Err(FileError::conflict());
            }
            if kind == ContentKind::Text {
                ReadResult::Text {
                    path,
                    mime_type,
                    size,
                    revision: revision.clone(),
                    text: String::from_utf8(bytes).map_err(|_| FileError::conflict())?,
                }
            } else {
                ReadResult::Image {
                    path,
                    mime_type,
                    size,
                    revision: revision.clone(),
                    bytes,
                }
            }
        };
        let editor = Self {
            target,
            file,
            snapshot,
            revision,
        };
        editor.verify(service, location_for(&document))?;
        Ok((editor, document))
    }

    pub fn write(&mut self, backend: &EditorBackend, input: &WriteText) -> Result<WriteResult> {
        self.replace(
            backend,
            &input.path,
            &input.revision,
            input.text.as_bytes(),
            None,
        )
    }

    pub fn write_image(
        &mut self,
        backend: &EditorBackend,
        input: &ImageWrite,
    ) -> Result<WriteResult> {
        self.replace(
            backend,
            &input.path,
            &input.revision,
            &input.bytes,
            Some(&input.mime_type),
        )
    }

    fn verify(&self, service: &FileService, path: &str) -> Result<()> {
        service
            .recheck_parent(path, &self.target)
            .map_err(|_| FileError::conflict())?;
        let current = content::open_regular(&self.target.parent, &self.target.name)
            .map_err(|_| FileError::conflict())?;
        if !content::same_version(&self.snapshot, &content::snapshot(&self.file)?)
            || !content::same_held_file(&self.file, &current)?
        {
            return Err(FileError::conflict());
        }
        Ok(())
    }

    fn replace(
        &mut self,
        backend: &EditorBackend,
        path: &str,
        revision: &str,
        bytes: &[u8],
        mime: Option<&str>,
    ) -> Result<WriteResult> {
        if self.target.absolute.as_os_str() != path || revision != self.revision {
            return Err(FileError::conflict());
        }
        if mime.is_none() && bytes.contains(&0) {
            return Err(FileError::new(ErrorCode::Validation, "Text contains NUL."));
        }
        let service = &backend.capability;
        let check = || self.verify(service, path);
        let request = BeginWrite {
            path: path.into(),
            size: bytes.len() as u64,
            mode: WriteMode::Replace,
            revision: Some(revision.into()),
            content: if mime.is_some() {
                TransferContent::Image
            } else {
                TransferContent::Text
            },
            mime_type: mime.map(str::to_owned),
        };
        let mut stage = StagedWrite::begin(service, &request, TRANSFER_MAX_BYTES, &check)?;
        for (index, bytes) in bytes.chunks(CHUNK_BYTES).enumerate() {
            stage.write(
                service,
                &WriteChunk {
                    transfer_id: Uuid::nil(),
                    offset: (index * CHUNK_BYTES) as u64,
                    data: bytes.to_vec(),
                },
                &check,
            )?;
        }
        // Keep the actual staged object, not whatever the name resolves to after rename.
        let published = stage.file.try_clone()?;
        let result = stage.commit(service, TRANSFER_MAX_BYTES, &check)?;
        self.snapshot = content::snapshot(&published).map_err(|_| FileError::new(
            ErrorCode::StorageFailed, "Save completed but its new baseline could not be verified. Reload before saving again."))?;
        self.file = published;
        self.revision.clone_from(&result.revision);
        Ok(result)
    }
}

#[cfg(test)]
mod tests;

fn location_for(document: &ReadResult) -> &str {
    match document {
        ReadResult::Text { path, .. }
        | ReadResult::Image { path, .. }
        | ReadResult::Binary { path, .. } => path,
    }
}
