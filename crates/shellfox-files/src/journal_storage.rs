//! Same V2 metadata CAS publisher, with original capture installed before native creation.
use crate::{
    content,
    journal::{invalid, JournalDocument, StagingJournal, JOURNAL_BYTES},
    platform, upload_private, *,
};
use cap_std::fs::{Dir, File};
use std::{io::Write, path::PathBuf};
pub(crate) struct MetadataStage {
    directory: Dir,
    name: PathBuf,
    file: Option<File>,
    published: bool,
}
fn check_version(
    expected: &Option<content::Snapshot>,
    target: &crate::paths::Target,
) -> Result<()> {
    match content::open_regular(&target.parent, &target.name) {
        Ok(file) => {
            let current = content::snapshot(&file)?;
            if !expected
                .as_ref()
                .is_some_and(|old| content::same_version(old, &current))
            {
                return Err(FileError::conflict());
            }
            Ok(())
        }
        Err(error) if error.code == ErrorCode::NotFound && expected.is_none() => Ok(()),
        Err(error) => Err(error),
    }
}
impl StagingJournal {
    pub(crate) fn persist(&mut self, _mutation: &std::sync::MutexGuard<'static, ()>) -> Result<()> {
        self.root.check()?;
        if let Some(previous) = &self.metadata_stage {
            return Err(FileError::new(
                ErrorCode::StorageFailed,
                if previous.published {
                    "Journal publication needs verification; no mutation was retried."
                } else {
                    "Journal preparation outcome is retained; no mutation was retried."
                },
            ));
        }
        let bytes = serde_json::to_vec(&JournalDocument {
            version: 2,
            entries: self.entries.values().cloned().collect(),
        })
        .map_err(|_| invalid())?;
        if bytes.len() > JOURNAL_BYTES {
            return Err(invalid());
        }
        let target = self.root.target("file-transfer-stages-v2.json")?;
        check_version(&self.expected, &target)?;
        let name = PathBuf::from(format!(
            ".file-transfer-stages-v2-{}.next",
            uuid::Uuid::new_v4()
        ));
        self.metadata_stage = Some(MetadataStage {
            directory: target.parent.try_clone()?,
            name,
            file: None,
            published: false,
        });
        let stage = self.metadata_stage.as_mut().ok_or_else(invalid)?;
        stage.file = Some(
            stage
                .directory
                .open_with(&stage.name, &platform::stage_options())?,
        );
        let file = stage.file.as_mut().ok_or_else(invalid)?;
        upload_private::protect_file(file)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        self.root.check()?;
        let current = content::open_regular(&stage.directory, &stage.name)?;
        if !content::same_held_file(&current, file)? {
            return Err(FileError::conflict());
        }
        drop(current);
        check_version(&self.expected, &target)?;
        self.root.check()?;
        let published = crate::HandleIdentity::from_cap_file(file)?;
        crate::journal_publish::replace(&target.parent, &stage.name, &target.name, file)?;
        stage.published = true; // actual FD still rooted in original metadata capture through readback
        #[cfg(unix)]
        crate::platform_directory_sync::sync_directory(&target.parent)?;
        self.root.check()?;
        let mut current = content::open_regular(&target.parent, &target.name)?;
        if crate::HandleIdentity::from_cap_file(&current)? != published {
            return Err(FileError::conflict());
        }
        let (actual, snapshot) = content::read(&mut current, JOURNAL_BYTES)?;
        if actual != bytes {
            return Err(FileError::conflict());
        }
        self.expected = Some(snapshot);
        self.metadata_stage = None; // known intended metadata, close only, never unlink
        Ok(())
    }
}
