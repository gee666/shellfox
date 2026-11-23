use crate::{content, paths, upload_inbox::SavedUpload, *};
use std::{path::Path, time::SystemTime};
use uuid::Uuid;

impl UploadInbox {
    pub(crate) fn restore(&mut self) -> Result<()> {
        self.verify_root()?;
        let (base, relative, _) = self.capability.base(".")?;
        let root = paths::descend(base, &relative)?;
        // Uploads from an earlier run are left alone but count toward the quota: a name or
        // mtime alone does not prove this inbox wrote them.
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
    /// A fresh private directory per upload. An existing name is a conflict, never adopted.
    pub(crate) fn create_upload_directory(&self, name: &str) -> Result<crate::private::PrivateDir> {
        self.verify_root()?;
        let target = self.capability.target(name)?;
        self.capability.recheck_parent(name, &target)?;
        crate::private::create_dir(
            &target.parent.try_clone()?.into_std_file(),
            target.name.as_os_str(),
        )
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
