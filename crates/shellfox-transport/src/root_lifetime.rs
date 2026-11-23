use crate::{HelperError, OwnershipRootIdentity};
use std::{
    fs::File,
    sync::{Arc, Mutex},
};
struct Originals {
    directories: Mutex<Vec<File>>,
}
/// Shared lifetime custody ONLY. No path, authority, VFS or confinement claim.
/// Original handles append under preparation's registration/seal lock; all
/// previously registered controllers share this SAME allocation.
#[derive(Clone)]
pub struct RootLifetimeGuard(Arc<Originals>);
impl std::fmt::Debug for RootLifetimeGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RootLifetimeGuard([original directories only])")
    }
}
impl RootLifetimeGuard {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Originals {
            directories: Mutex::new(Vec::new()),
        }))
    }
    pub(crate) fn append(
        &self,
        files: Vec<File>,
    ) -> Result<Vec<OwnershipRootIdentity>, HelperError> {
        let mut originals = self.0.directories.lock().unwrap_or_else(|e| e.into_inner());
        if files.is_empty() || originals.len() + files.len() > 256 {
            return Err(HelperError::InvalidRequest);
        }
        let identities = files
            .iter()
            .map(OwnershipRootIdentity::from_file)
            .collect::<Result<Vec<_>, _>>()?;
        originals.extend(files);
        Ok(identities)
    }
}
