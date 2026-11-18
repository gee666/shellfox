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
    journal: Option<crate::journal::SharedJournal>,
    custody: Option<std::sync::Arc<crate::user_stage_custody::UserStageCustody>>,
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
pub(crate) fn require_user_stage_contract(service: &FileService) -> Result<()> {
    if service.is_anchored_relative() {
        return Err(FileError::new(ErrorCode::Unsupported,"Native user-stage writes require separately accepted target and recovery authority contracts."));
    }
    Ok(())
}
pub(crate) fn absent(target: &Target) -> Result<()> {
    match target.parent.symlink_metadata(&target.name) {
        Ok(_) => Err(FileError::conflict()),
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
        Self::begin_inner(service, input, max, check, None, None)
    }
    pub fn begin_with_journal(
        service: std::sync::Arc<FileService>,
        input: &BeginWrite,
        max: u64,
        check: &dyn Fn() -> Result<()>,
        journal: Option<crate::journal::SharedJournal>,
    ) -> Result<Self> {
        require_user_stage_contract(&service)?;
        if journal.is_none() {
            return Err(crate::journal::invalid());
        }
        Self::begin_inner(&service, input, max, check, journal, Some(service.clone()))
    }
    fn begin_inner(
        service: &FileService,
        input: &BeginWrite,
        max: u64,
        check: &dyn Fn() -> Result<()>,
        journal: Option<crate::journal::SharedJournal>,
        authority: Option<std::sync::Arc<FileService>>,
    ) -> Result<Self> {
        require_user_stage_contract(service)?;
        validate(input, max)?;
        check()?;
        service.authorized(true)?;
        let target = service.target(&input.path)?;
        #[cfg(unix)]
        crate::private_verify::parent(&target.parent.try_clone()?.into_std_file())?;
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
        let stage_path = target.absolute.with_file_name(&name);
        let custody = if let Some(journal) = &journal {
            let authority = authority.ok_or_else(crate::journal::invalid)?;
            if !std::ptr::eq(authority.as_ref(), service) {
                return Err(FileError::denied());
            }
            let mutation = FileService::mutation_lock()?;
            Some(
                journal
                    .lock()
                    .map_err(|_| crate::journal::invalid())?
                    .reserve_stage(authority, &input.path, &target, &name, &mutation)?,
            )
        } else {
            None
        };
        let file = if let Some(owner) = &custody {
            owner.create(check)?
        } else {
            target.parent.open_with(&name, &platform::stage_options())?
        };
        let stage = Self {
            target,
            input: input.clone(),
            file,
            name,
            previous,
            offset: 0,
            hash: Sha256::new(),
            last: None,
            journal,
            custody,
            publication_entered: false,
        };
        // A broad parent ACL must never expose uploaded/editor bytes while staging.
        if stage.custody.is_none() {
            crate::upload_private::protect_file(&stage.file)?;
        }
        if let Some(journal) = &stage.journal {
            let mutation = FileService::mutation_lock()?;
            journal
                .lock()
                .map_err(|_| crate::journal::invalid())?
                .phase(&stage_path, crate::journal::StagePhase::Owned, &mutation)?;
        }
        check()?;
        Ok(stage)
    }
    pub fn verify(&self, service: &FileService) -> Result<()> {
        require_user_stage_contract(service)?;
        service.recheck_parent(&self.input.path, &self.target)?;
        if let Some(custody) = &self.custody {
            custody.check()?;
        }
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
        require_user_stage_contract(service)?;
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
#[path = "staged_write_cleanup.rs"]
mod cleanup;
#[path = "staged_write_commit.rs"]
mod commit;

#[cfg(test)]
#[path = "staged_write/native_gate_tests.rs"]
mod native_gate_tests;
#[cfg(test)]
mod tests;
