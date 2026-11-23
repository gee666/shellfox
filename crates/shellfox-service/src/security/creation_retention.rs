//! Private directories created during native preparation stay open, and on Windows pinned
//! against rename, until the preparation releases them.
use anyhow::{bail, Result};
use shellfox_files::private::PrivateDir;
use std::{
    fs::File,
    sync::{Arc, Mutex},
};

type Slot = Arc<Mutex<Option<PrivateDir>>>;

#[derive(Default)]
pub(crate) struct NativeCreationRetention {
    created: Mutex<Vec<Slot>>,
}
impl std::fmt::Debug for NativeCreationRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeCreationRetention")
    }
}
#[derive(Clone)]
pub(crate) struct CreationHandle {
    slot: Slot,
}
impl NativeCreationRetention {
    pub fn retain(&self, created: shellfox_files::Result<PrivateDir>) -> Result<CreationHandle> {
        let slot = Arc::new(Mutex::new(Some(created?)));
        self.created
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(slot.clone());
        Ok(CreationHandle { slot })
    }
    /// Closes every created directory handle. The directories themselves stay.
    pub fn release(&self) {
        for slot in self
            .created
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
        {
            slot.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take();
        }
    }
}
impl CreationHandle {
    fn with<T>(&self, operation: impl FnOnce(&PrivateDir) -> Result<T>) -> Result<T> {
        let slot = self
            .slot
            .lock()
            .map_err(|_| anyhow::anyhow!("Native creation slot poisoned"))?;
        match slot.as_ref() {
            Some(created) => operation(created),
            None => bail!("Created directory was already released"),
        }
    }
    pub fn clone_file(&self) -> Result<File> {
        self.with(|created| Ok(created.as_file().try_clone()?))
    }
    pub fn verify(&self) -> Result<()> {
        self.with(|created| Ok(created.verify()?))
    }
}
