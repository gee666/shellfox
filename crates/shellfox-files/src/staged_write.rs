use crate::{
    assets, content,
    paths::{self, Target},
    platform, stream_io, *,
};
use cap_std::fs::File;
use sha2::{Digest, Sha256};
use std::{
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
};
use uuid::Uuid;

pub(crate) struct StagedWrite {
    pub target: Target,
    pub input: BeginWrite,
    pub file: File,
    name: PathBuf,
    previous: Option<(File, content::Snapshot)>,
    offset: u64,
    hash: Sha256,
    last: Option<(u64, usize, String)>,
    /// Set after a successful rename; from then on the stage name is the target.
    publication_entered: bool,
}
fn invalid(message: &str) -> FileError {
    FileError::new(ErrorCode::Validation, message)
}

pub(crate) fn validate(input: &BeginWrite, max: u64) -> Result<()> {
    paths::validate(&input.path)?;
    if input.size > max
        || input.size > TRANSFER_MAX_BYTES
        || (input.content == TransferContent::Text && input.size > TRANSFER_TEXT_BYTES)
        || (input.content == TransferContent::Image && input.size > TRANSFER_IMAGE_BYTES)
        || (input.mode == WriteMode::Replace) != input.revision.is_some()
        || (input.content == TransferContent::Image) != input.mime_type.is_some()
        || input
            .mime_type
            .as_deref()
            .is_some_and(|mime| !assets::editable_image(mime))
    {
        return Err(invalid(
            "Invalid transfer size, content, mode or image MIME type.",
        ));
    }
    if let Some(revision) = &input.revision {
        content::validate_revision(revision)?;
    }
    Ok(())
}
pub(crate) fn absent(target: &Target) -> Result<()> {
    match target.parent.symlink_metadata(&target.name) {
        Ok(_) => Err(FileError::exists()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

impl StagedWrite {
    pub fn begin(
        service: &FileService,
        input: &BeginWrite,
        max: u64,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        validate(input, max)?;
        check()?;
        service.authorized(true)?;
        let target = service.target(&input.path)?;
        let previous = if input.mode == WriteMode::Replace {
            let mut file = content::open_regular(&target.parent, &target.name)?;
            let previous = stream_io::inspect(&mut file, &target.absolute, max, check)?;
            if previous.snapshot.metadata.permissions().readonly() {
                return Err(FileError::denied());
            }
            if input.revision.as_deref() != Some(&previous.revision) {
                return Err(FileError::conflict());
            }
            if input.content == TransferContent::Image
                && (previous.kind != ContentKind::Image
                    || input.mime_type.as_deref() != Some(&previous.mime_type))
            {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Image editing requires an existing matching image.",
                ));
            }
            if input.content == TransferContent::Text && previous.kind != ContentKind::Text {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Only UTF-8 text files can be edited.",
                ));
            }
            Some((file, previous.snapshot))
        } else {
            absent(&target)?;
            None
        };
        let name = PathBuf::from(format!(".shellfox-transfer-{}.partial", Uuid::new_v4()));
        // Owner-only from creation, even under a broad parent ACL.
        let file = File::from_std(crate::private::create_stage(
            &target.parent.try_clone()?.into_std_file(),
            name.as_os_str(),
        )?);
        let stage = Self {
            target,
            input: input.clone(),
            file,
            name,
            previous,
            offset: 0,
            hash: Sha256::new(),
            last: None,
            publication_entered: false,
        };
        check()?;
        Ok(stage)
    }
    pub fn verify(&self, service: &FileService) -> Result<()> {
        service.recheck_parent(&self.input.path, &self.target)?;
        let opened = content::open_regular(&self.target.parent, &self.name)?;
        if !content::same_held_file(&opened, &self.file)?
            || self.file.metadata()?.len() != self.offset
        {
            return Err(FileError::conflict());
        }
        Ok(())
    }
    pub fn write(
        &mut self,
        service: &FileService,
        input: &WriteChunk,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<TransferProgress> {
        check()?;
        self.verify(service)?;
        if input.data.is_empty() || input.data.len() > CHUNK_BYTES {
            return Err(invalid("Invalid upload chunk size."));
        }
        let hash = content::revision(&input.data);
        if self
            .last
            .as_ref()
            .is_some_and(|last| last == &(input.offset, input.data.len(), hash.clone()))
        {
            check()?;
            return Ok(TransferProgress {
                offset: self.offset,
            });
        }
        if input.offset != self.offset || input.data.len() as u64 > self.input.size - self.offset {
            return Err(invalid("Unexpected upload offset or size."));
        }
        self.file.seek(SeekFrom::Start(self.offset))?;
        let mut written = 0;
        while written < input.data.len() {
            check()?;
            let count = self.file.write(&input.data[written..])?;
            if count == 0 {
                return Err(FileError::new(
                    ErrorCode::StorageFailed,
                    "Empty stage write.",
                ));
            }
            written += count;
        }
        check()?;
        self.hash.update(&input.data);
        self.last = Some((self.offset, input.data.len(), hash));
        self.offset += written as u64;
        Ok(TransferProgress {
            offset: self.offset,
        })
    }
}
mod cleanup;
mod commit;

#[cfg(test)]
mod tests;
