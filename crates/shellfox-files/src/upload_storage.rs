use crate::{content, paths, upload_inbox::SavedUpload, *};
use std::{path::Path, time::SystemTime};
use uuid::Uuid;

impl UploadInbox {
    pub(crate) fn restore(&mut self) -> Result<()> {
        self.verify_root()?;
        let (base, relative, _) = self.capability.base(".")?;
        let root = paths::descend(base, &relative)?;
        // Restart has no original stage/publication FD. Preserve every previous UUID entry,
        // including apparent completed filenames. Names/fresh IDs/mtime cannot mint TTL cleanup.
        for entry in root.entries()? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| FileError::denied())?;
            if name == ".shellfox-transfer-meta" {
                continue;
            }
            let id = Uuid::parse_str(&name).map_err(|_| FileError::denied())?;
            if id.to_string() != name || self.preserved_count >= UPLOAD_ENTRIES {
                return Err(FileError::denied());
            }
            let info = entry.metadata()?;
            paths::reject_link(&info)?;
            if !info.is_dir() {
                return Err(FileError::denied());
            }
            let directory = paths::descend(root.try_clone()?, Path::new(&name))?;
            let mut entries = directory.entries()?;
            let first = entries.next().transpose()?;
            if entries.next().is_some() {
                return Err(FileError::denied());
            }
            if let Some(file) = first {
                let info = file.metadata()?;
                paths::reject_link(&info)?;
                if !info.is_file() {
                    return Err(FileError::denied());
                }
                self.preserved_bytes = self.preserved_bytes.saturating_add(info.len());
            }
            self.preserved_count += 1;
        }
        Ok(())
    }
    pub(crate) fn create_upload_directory(&self, name: &str) -> Result<CreatedPrivate> {
        let _mutation = FileService::mutation_lock()?;
        let journal = self
            .registry
            .journal
            .as_ref()
            .ok_or_else(crate::journal::invalid)?;
        let journal = journal.lock().map_err(|_| crate::journal::invalid())?;
        journal.root.check()?;
        if journal.entries.len() >= crate::journal::JOURNAL_ENTRIES {
            return Err(crate::journal::invalid());
        }
        self.verify_root()?;
        let target = self.capability.target(name)?;
        self.capability.recheck_parent(name, &target)?;
        // Generic strict private-parent creator, exclusive birth, no collision adoption/repair.
        // Keep this actual receipt until the V2 user stage has captured that same parent.
        match create_private_directory_at(
            &target.parent.try_clone()?.into_std_file(),
            target.name.as_os_str(),
        ) {
            Ok(created) => Ok(created),
            Err(failure) => {
                let (error, mut custody) = failure.into_parts();
                if let Some(held) = custody.as_mut() {
                    held.preserve_stale();
                }
                Err(error)
            }
        }
    }
    pub(crate) fn remove_empty_directory(&self, id: Uuid) -> Result<()> {
        self.verify_root()?;
        match self.capability.delete(&Delete {
            path: id.to_string(),
            kind: DeleteKind::EmptyDirectory,
        }) {
            Ok(()) => Ok(()),
            Err(error) if error.code == ErrorCode::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
    pub(crate) fn remove_saved(&self, id: Uuid, entry: &SavedUpload) -> Result<()> {
        self.verify_root()?;
        let target = self.capability.target(&entry.relative)?;
        let file = content::open_regular(&target.parent, &target.name)?;
        if HandleIdentity::from_cap_file(&file)? != entry.identity {
            return Err(FileError::conflict());
        }
        drop(file);
        drop(target);
        self.capability.delete(&Delete {
            path: entry.relative.clone(),
            kind: DeleteKind::File,
        })?;
        self.remove_empty_directory(id)
    }
    pub(crate) fn expire_saved(&mut self) -> Result<()> {
        let now = SystemTime::now();
        let expired: Vec<_> = self
            .saved
            .iter()
            .filter(|(_, entry)| {
                now.duration_since(entry.touched).unwrap_or_default() > self.limits.saved_ttl
            })
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(entry) = self.saved.get(&id) {
                self.remove_saved(id, entry)?;
            }
            self.saved.remove(&id);
        }
        Ok(())
    }
}
