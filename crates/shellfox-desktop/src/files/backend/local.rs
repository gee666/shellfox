use super::*;

impl LocalDocumentBackend {
    pub(super) fn list_location(&self, location: &Location) -> shellfox_files::Result<FileList> {
        self.service.list(&self.service.browser_existing(location)?)
    }

    pub(super) fn read_document(
        &self,
        path: &str,
        cancel: &EditorCancellation,
    ) -> Result<ReadResult> {
        Ok(self.session.read_document(
            &self.service.browser_existing(&self.location(path)?)?,
            cancel,
        )?)
    }

    pub(super) fn lookup_path(&self, path: &str) -> Result<Target> {
        let location = self.location(path)?;
        let initial_error = match self.list_location(&location) {
            Ok(listing) => return Ok(Target::Directory(listing)),
            Err(error) if error.code == ErrorCode::NotFound => error,
            Err(error) if error.code == ErrorCode::Validation => {
                // The capability walker reports a regular leaf as a non-directory component.
                if !self
                    .service
                    .browser_existing(&location)
                    .and_then(|resolved| self.service.stat(&resolved))
                    .is_ok_and(|stat| stat.kind == EntryKind::File)
                {
                    return Err(error.into());
                }
                error
            }
            Err(error) => return Err(error.into()),
        };
        let path = std::path::Path::new(&location.path);
        let (Some(parent), Some(name)) = (
            path.parent().and_then(std::path::Path::to_str),
            path.file_name().and_then(std::ffi::OsStr::to_str),
        ) else {
            return Err(initial_error.into());
        };
        match self.list_location(&Location {
            path: if parent.is_empty() {
                ".".into()
            } else {
                parent.into()
            },
        }) {
            Ok(parent) => {
                let entry = parent.entries.iter().find(|entry| {
                    entry.name == name
                        || cfg!(windows) && entry.name.to_lowercase() == name.to_lowercase()
                });
                if let Some(entry) = entry {
                    if matches!(entry.kind, EntryKind::File | EntryKind::Symlink) {
                        let path = entry.path.clone();
                        return Ok(Target::File { parent, path });
                    }
                } else if parent.truncated {
                    anyhow::bail!("Folder listing is incomplete. Choose the file from its folder.");
                }
            }
            Err(error) if error.code == ErrorCode::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if initial_error.code == ErrorCode::NotFound {
            Ok(Target::Missing(location.path))
        } else {
            Err(initial_error.into())
        }
    }

    pub(super) fn location(&self, path: &str) -> Result<Location> {
        let input = Location { path: path.into() };
        if std::path::Path::new(path).is_absolute() {
            return Ok(input);
        }
        Ok(self
            .service
            .resolve_location(&input, &Self::home_context()?)?)
    }

    pub(super) fn home_context() -> Result<FileContext> {
        let home = std::env::home_dir()
            .and_then(|path| path.into_os_string().into_string().ok())
            .context("The host home directory is unavailable.")?;
        Ok(FileContext {
            cwd: home.clone(),
            home: Some(home),
        })
    }
}
