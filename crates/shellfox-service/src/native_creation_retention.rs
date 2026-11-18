//! Bounded filesystem custody inside the ORIGINAL native preparation owner.
use anyhow::{bail, Result};
use shellfox_files::{CreatedPrivate, CreationCustody, HandleIdentity, PrivateCreationFailure};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    sync::{Arc, Mutex},
};
const LIMIT: usize = 258; // <=256 root components plus the two fixed layout children.

enum Outcome {
    Reserved,
    Created(CreatedPrivate),
    Failed(Option<CreationCustody>),
    PreservedUnknown,
    PreservedCreated(HandleIdentity),
}
struct Slot {
    _parent: File,
    _parent_id: HandleIdentity,
    _leaf: OsString,
    outcome: Outcome,
}
#[derive(Default)]
pub(crate) struct NativeCreationRetention {
    slots: Mutex<Vec<Arc<Mutex<Slot>>>>,
}
impl std::fmt::Debug for NativeCreationRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeCreationRetention([original filesystem custody])")
    }
}
#[derive(Clone)]
pub(crate) struct CreationHandle {
    owner: Arc<NativeCreationRetention>,
    slot: Arc<Mutex<Slot>>,
}
impl NativeCreationRetention {
    pub fn reserve(self: &Arc<Self>, parent: &File, leaf: &OsStr) -> Result<CreationHandle> {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if slots.len() >= LIMIT {
            bail!("Native creation retention bound reached");
        }
        // All allocations/parent capture happen BEFORE a native create syscall.
        slots.try_reserve(1)?;
        let slot = Arc::new(Mutex::new(Slot {
            _parent: parent.try_clone()?,
            _parent_id: HandleIdentity::from_file(parent)?,
            _leaf: leaf.to_owned(),
            outcome: Outcome::Reserved,
        }));
        slots.push(slot.clone());
        Ok(CreationHandle {
            owner: self.clone(),
            slot,
        })
    }
    pub fn preserve_stale(&self) -> Result<()> {
        let slots = self
            .slots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for entry in slots.iter() {
            let mut slot = entry.lock().unwrap_or_else(|poison| poison.into_inner());
            match &mut slot.outcome {
                Outcome::Failed(Some(custody)) => {
                    custody.preserve_stale();
                }
                Outcome::Created(_) => {
                    let old = std::mem::replace(&mut slot.outcome, Outcome::Reserved);
                    let Outcome::Created(created) = old else {
                        unreachable!("matched created slot")
                    };
                    let identity = created.identity().clone();
                    // Explicitly close only this created handle. Retain original
                    // parent/full ID evidence. Never unlink or claim NoCreate.
                    drop(created.into_file());
                    slot.outcome = Outcome::PreservedCreated(identity);
                }
                Outcome::Reserved => slot.outcome = Outcome::PreservedUnknown,
                _ => {}
            }
        }
        Ok(())
    }
    pub fn failure(self: &Arc<Self>, error: anyhow::Error) -> anyhow::Error {
        NativePreparationFailure {
            error,
            _retained: self.clone(),
        }
        .into()
    }
}
impl CreationHandle {
    /// Prepared for the coordinated breaking creator return. Both success and
    /// retained failure are moved into this pre-reserved ORIGINAL owner slot.
    pub fn finish(
        self,
        result: std::result::Result<CreatedPrivate, PrivateCreationFailure>,
    ) -> Result<Self> {
        match result {
            Ok(created) => {
                self.slot
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .outcome = Outcome::Created(created);
                Ok(self)
            }
            Err(failure) => {
                let (error, custody) = failure.into_parts();
                self.slot
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .outcome = Outcome::Failed(custody);
                Err(self.owner.failure(error.into()))
            }
        }
    }
    pub fn with_created<T>(
        &self,
        operation: impl FnOnce(&CreatedPrivate) -> Result<T>,
    ) -> Result<T> {
        let slot = self
            .slot
            .lock()
            .map_err(|_| anyhow::anyhow!("Native creation slot poisoned"))?;
        let Outcome::Created(created) = &slot.outcome else {
            bail!("Created receipt is not live");
        };
        operation(created)
    }
    pub fn clone_file(&self) -> Result<File> {
        let slot = self
            .slot
            .lock()
            .map_err(|_| anyhow::anyhow!("Native creation slot poisoned"))?;
        let Outcome::Created(created) = &slot.outcome else {
            bail!("Creation handle is not a verified live object");
        };
        Ok(created.as_file().try_clone()?)
    }
    pub fn verify_binding(&self) -> Result<()> {
        let slot = self
            .slot
            .lock()
            .map_err(|_| anyhow::anyhow!("Native creation slot poisoned"))?;
        match &slot.outcome {
            Outcome::Created(created) => Ok(created.verify_binding()?),
            Outcome::PreservedCreated(identity) => {
                let _ = identity;
                bail!("Created directory was explicitly preserved");
            }
            _ => bail!("Creation validation did not complete"),
        }
    }
}
struct NativePreparationFailure {
    error: anyhow::Error,
    _retained: Arc<NativeCreationRetention>,
}
impl std::fmt::Debug for NativePreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePreparationFailure([retained filesystem custody])")
    }
}
impl std::fmt::Display for NativePreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}; filesystem creation evidence retained, not a no-create proof",
            self.error
        )
    }
}
impl std::error::Error for NativePreparationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}
