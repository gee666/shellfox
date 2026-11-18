use crate::{
    content,
    paths::{self, Target},
    platform,
    service::kind,
    *,
};
use cap_std::fs::File;
use std::{
    io::Write,
    path::{Component, PathBuf},
};
use uuid::Uuid;

struct Stage<'a> {
    parent: &'a cap_std::fs::Dir,
    name: PathBuf,
    file: File,
    publication_entered: bool,
}

impl Stage<'_> {
    fn verify(&self) -> Result<()> {
        let actual = content::open_regular(self.parent, &self.name)?;
        if !content::same_held_file(&actual, &self.file)? {
            return Err(FileError::conflict());
        }
        Ok(())
    }
}

impl Drop for Stage<'_> {
    fn drop(&mut self) {
        // Once namespace publication may have occurred, neither failure nor Drop grants cleanup.
        if self.publication_entered {
            return;
        }
        // Do not remove a stage path now occupied by somebody else's entry.
        if self.verify().is_ok() {
            let _ = self.parent.remove_file(&self.name);
        }
    }
}

impl FileService {
    pub fn create_file(&self, input: &CreateFile) -> Result<WriteResult> {
        content::validate_text(&input.text)?;
        self.save(&input.path, &input.text, None, || {})
    }

    pub fn write_text(&self, input: &WriteText) -> Result<WriteResult> {
        content::validate_text(&input.text)?;
        content::validate_revision(&input.revision)?;
        self.save(&input.path, &input.text, Some(&input.revision), || {})
    }

    fn save(
        &self,
        path: &str,
        text: &str,
        revision: Option<&str>,
        before_commit: impl FnOnce(),
    ) -> Result<WriteResult> {
        let _guard = Self::mutation_lock()?;
        self.authorized(true)?;
        let target = self.target(path)?;
        #[cfg(unix)]
        crate::private_verify::parent(&target.parent.try_clone()?.into_std_file())?;
        let previous = if let Some(expected) = revision {
            let mut file = content::open_regular(&target.parent, &target.name)?;
            let (bytes, info) = content::read(&mut file, TEXT_BYTES)?;
            content::text(&bytes)?;
            if info.metadata.permissions().readonly() {
                return Err(FileError::denied());
            }
            if content::revision(&bytes) != expected {
                return Err(FileError::conflict());
            }
            Some((file, info))
        } else {
            require_absent(&target)?;
            None
        };
        let stage_name = PathBuf::from(format!(".shellfox-save-{}", Uuid::new_v4()));
        let mut stage = Stage {
            parent: &target.parent,
            name: stage_name.clone(),
            publication_entered: false,
            file: target
                .parent
                .open_with(&stage_name, &platform::stage_options())?,
        };
        crate::upload_private::protect_file(&stage.file)?;
        stage.file.write_all(text.as_bytes())?;
        if let Some((file, _)) = &previous {
            platform::preserve_permissions(file, &stage.file)?;
        }
        stage.file.sync_all()?;
        before_commit();
        self.recheck_parent(path, &target)?;
        stage.verify()?;
        let (staged_bytes, _) = content::read(
            &mut content::open_regular(&target.parent, &stage.name)?,
            TEXT_BYTES,
        )?;
        if staged_bytes != text.as_bytes() {
            return Err(FileError::conflict());
        }
        if let Some((_, info)) = &previous {
            verify_previous(&target, info, revision.unwrap_or_default())?;
        } else {
            require_absent(&target)?;
        }
        self.authorized(true)?;
        let written = WriteResult {
            path: paths::display(&target.absolute)?,
            size: text.len() as u64,
            revision: content::revision(text.as_bytes()),
        };
        let source = Target {
            parent: target.parent.try_clone()?,
            name: stage.name.clone(),
            absolute: target.absolute.with_file_name(&stage.name),
        };
        let check = || {
            self.recheck_parent(path, &target)?;
            self.authorized(true)
        };
        stage.publication_entered = true;
        if previous.is_some() {
            platform::rename_user_replace_checked(&source, &target, &stage.file, &check)?;
        } else {
            platform::rename_user_new_checked(&source, &target, &stage.file, &check)?;
        }
        #[cfg(unix)]
        crate::platform_directory_sync::sync_directory(&target.parent)?;
        Ok(written)
    }

    pub(crate) fn recheck_parent(&self, path: &str, original: &Target) -> Result<()> {
        let now = self.target(path)?;
        if HandleIdentity::from_directory(&now.parent)?
            != HandleIdentity::from_directory(&original.parent)?
        {
            return Err(FileError::conflict());
        }
        Ok(())
    }

    /// Recursive, idempotent directory creation without following any existing link.
    pub fn create_directory(&self, input: &Location) -> Result<()> {
        let _guard = Self::mutation_lock()?;
        self.authorized(true)?;
        let (mut dir, relative, _) = self.base(&input.path)?;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                if component == Component::CurDir {
                    continue;
                }
                return Err(FileError::denied());
            };
            match dir.create_dir(name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            dir = paths::descend(dir, std::path::Path::new(name))?;
        }
        Ok(())
    }

    /// Same-filesystem rename with atomic no-overwrite semantics, including directories.
    pub fn rename(&self, input: &Rename) -> Result<()> {
        let _guard = Self::mutation_lock()?;
        self.authorized(true)?;
        let source = self.target(&input.path)?;
        let destination = self.target(&input.destination)?;
        let info = source.parent.symlink_metadata(&source.name)?;
        paths::reject_link(&info)?;
        if !info.is_file() && !info.is_dir() {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Only regular files and directories can be renamed.",
            ));
        }
        require_absent(&destination)?;
        self.recheck_parent(&input.path, &source)?;
        self.recheck_parent(&input.destination, &destination)?;
        self.authorized(true)?;
        platform::rename_new(&source, &destination)
    }

    /// Deletes one regular file or one empty directory. Never follows links or recurses.
    pub fn delete(&self, input: &Delete) -> Result<()> {
        let _guard = Self::mutation_lock()?;
        self.authorized(true)?;
        let target = self.target(&input.path)?;
        let metadata = target.parent.symlink_metadata(&target.name)?;
        paths::reject_link(&metadata)?;
        let expected = match input.kind {
            DeleteKind::File => EntryKind::File,
            DeleteKind::EmptyDirectory => EntryKind::Directory,
        };
        if kind(&metadata) != expected {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Delete kind does not match the selected entry.",
            ));
        }
        self.recheck_parent(&input.path, &target)?;
        self.authorized(true)?;
        match input.kind {
            DeleteKind::File => target.parent.remove_file(&target.name)?,
            DeleteKind::EmptyDirectory => target.parent.remove_dir(&target.name)?,
        }
        Ok(())
    }
}

fn require_absent(target: &Target) -> Result<()> {
    match target.parent.symlink_metadata(&target.name) {
        Ok(_) => Err(FileError::conflict()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn verify_previous(target: &Target, info: &content::Snapshot, expected: &str) -> Result<()> {
    let mut current = content::open_regular(&target.parent, &target.name)?;
    let (bytes, now) = content::read(&mut current, TEXT_BYTES)?;
    if !content::same_version(info, &now) || content::revision(&bytes) != expected {
        return Err(FileError::conflict());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
