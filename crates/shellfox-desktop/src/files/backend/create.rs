use super::*;
use shellfox_files::CreateFile;

impl DocumentBackend {
    /// Creates an empty file and returns its path. Never replaces an existing entry.
    pub(in crate::files) async fn create_file(&self, path: &str) -> Result<String> {
        self.check_current()?;
        let created = match self {
            Self::Local(local) => local.create_file(path),
            #[cfg(windows)]
            Self::Wsl(backend) => backend.create_file(path),
            Self::Peer(peer) => peer.create_file(path).await,
        };
        // Every backend reports an occupied create target as a conflict; say what it means here.
        created.map_err(|error| {
            if error
                .downcast_ref::<shellfox_files::FileError>()
                .is_some_and(|error| error.code == ErrorCode::FileConflict)
            {
                anyhow!("A file already exists at this destination.")
            } else {
                error
            }
        })
    }
}

impl LocalDocumentBackend {
    fn create_file(&self, path: &str) -> Result<String> {
        let location = self.service.browser_create_target(&self.location(path)?)?;
        Ok(self
            .service
            .create_file(&CreateFile {
                path: location.path,
                text: String::new(),
            })?
            .path)
    }
}
