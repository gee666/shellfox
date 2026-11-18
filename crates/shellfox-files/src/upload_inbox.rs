use crate::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Instant, SystemTime},
};
use uuid::Uuid;

pub struct UploadInbox {
    pub(crate) root: PathBuf,
    pub(crate) capability: Arc<FileService>,
    pub(crate) limits: UploadLimits,
    pub(crate) registry: TransferRegistry,
    pub(crate) pending: HashMap<Uuid, PendingUpload>,
    pub(crate) saved: HashMap<Uuid, SavedUpload>,
    pub(crate) disposed: bool,
    pub(crate) preserved_count: usize,
    pub(crate) preserved_bytes: u64,
}
#[derive(Clone)]
pub(crate) struct PendingUpload {
    pub actor: UploadActor,
    pub transfer_id: Uuid,
    pub file_name: String,
    pub size: u64,
    pub touched: Instant,
}
pub(crate) struct SavedUpload {
    pub relative: String,
    pub size: u64,
    pub touched: SystemTime,
    pub identity: HandleIdentity,
    pub completion: (UploadActor, UploadFinished),
}
impl UploadInbox {
    pub fn open(root: impl AsRef<Path>, limits: UploadLimits) -> Result<Self> {
        limits.validate()?;
        let root = root.as_ref().to_path_buf();
        crate::paths::validate(&crate::paths::display(&root)?)?;
        if !root.is_absolute() || root.file_name().is_none() {
            return Err(FileError::denied());
        }
        let issuer = Arc::new(FileService::local_desktop(Access::ReadWrite));
        let plan = issuer.plan_directory(&Location {
            path: crate::paths::display(&root)?,
        })?;
        let capability = match plan.create_private_approved_root() {
            Ok(root) => root,
            Err(mut failure) => {
                failure
                    .preserve_stale()
                    .map_err(|_| crate::journal::invalid())?;
                return Err(FileError::new(
                    failure.error().code,
                    &failure.error().message,
                ));
            }
        };
        let metadata_plan = capability.plan_directory(&Location {
            path: ".shellfox-transfer-meta".into(),
        })?;
        let metadata = match metadata_plan.create_private_approved_root() {
            Ok(root) => root,
            Err(mut failure) => {
                failure
                    .preserve_stale()
                    .map_err(|_| crate::journal::invalid())?;
                return Err(FileError::new(
                    failure.error().code,
                    &failure.error().message,
                ));
            }
        };
        let mut registry = TransferRegistry::new(TransferLimits {
            max_file_bytes: limits.max_file_bytes,
            idle_ttl: limits.active_ttl,
        })?;
        registry.staging_limit = (512 * 1024 * 1024).max(limits.max_file_bytes * 2);
        registry.initialize_journal_anchored(metadata)?;
        let mut inbox = Self {
            root,
            capability,
            limits,
            registry,
            pending: HashMap::new(),
            saved: HashMap::new(),
            disposed: false,
            preserved_count: 0,
            preserved_bytes: 0,
        };
        inbox.restore()?;
        inbox.expire_saved()?;
        // V2 owns live stage custody; prior unknown partials are preserved, never adopted.
        Ok(inbox)
    }
    pub fn root_path(&self) -> &Path {
        &self.root
    }
    pub fn usage(&self) -> UploadUsage {
        UploadUsage {
            active: self.pending.len(),
            saved: self.saved.len(),
            reserved_bytes: self.pending.values().map(|entry| entry.size).sum::<u64>()
                + self.saved.values().map(|entry| entry.size).sum::<u64>(),
        }
    }
    pub(crate) fn authorized(
        &self,
        actor: &UploadActor,
        auth: &dyn UploadAuthorization,
    ) -> Result<()> {
        if self.disposed {
            return Err(FileError::denied());
        }
        self.verify_root()?;
        auth.check(actor)
    }
    pub(crate) fn verify_root(&self) -> Result<()> {
        self.capability.require_path_resolution()?;
        self.capability.authorized(true)?;
        let (base, relative, _) = self.capability.base(".")?;
        let directory = crate::paths::descend(base, &relative)?;
        crate::private_verify::object(&directory.into_std_file(), PrivateKind::Directory)?;
        Ok(())
    }
    pub(crate) fn selected(&self, actor: &UploadActor, id: Uuid) -> Result<PendingUpload> {
        let entry = self
            .pending
            .get(&id)
            .ok_or_else(crate::transfer_registry::missing)?;
        if &entry.actor != actor {
            return Err(crate::transfer_registry::missing());
        }
        Ok(entry.clone())
    }
    pub fn cancel(&mut self, actor: &UploadActor, input: &UploadIdentity) -> Result<()> {
        let entry = self.selected(actor, input.upload_id)?;
        self.pending.remove(&input.upload_id);
        let _ = self.registry.cancel(
            &entry.actor.connection,
            &TransferIdentity {
                transfer_id: entry.transfer_id,
            },
        );
        self.remove_empty_directory(input.upload_id)
    }
    pub fn cancel_actor(&mut self, owner: &str) -> Result<()> {
        let ids: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, entry)| entry.actor.connection.owner() == owner)
            .map(|(id, _)| *id)
            .collect();
        self.registry.cancel_actor(owner);
        for id in ids {
            self.pending.remove(&id);
            self.remove_empty_directory(id)?;
        }
        Ok(())
    }
    /// The provider must validate all stored actors, not just one request's connection.
    pub fn sweep(&mut self, auth: &dyn UploadAuthorization) -> Result<()> {
        if self.disposed {
            return Err(FileError::denied());
        }
        let ids: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, entry)| {
                entry.touched.elapsed() > self.limits.active_ttl
                    || auth.check(&entry.actor).is_err()
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(entry) = self.pending.remove(&id) {
                let _ = self.registry.cancel(
                    &entry.actor.connection,
                    &TransferIdentity {
                        transfer_id: entry.transfer_id,
                    },
                );
                self.remove_empty_directory(id)?;
            }
        }
        self.expire_saved()
    }
    pub fn dispose(&mut self) -> Result<()> {
        self.disposed = true;
        self.registry.dispose();
        let ids: Vec<_> = self.pending.keys().copied().collect();
        self.pending.clear();
        for id in ids {
            self.remove_empty_directory(id)?;
        }

        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct UploadUsage {
    pub active: usize,
    pub saved: usize,
    pub reserved_bytes: u64,
}
impl Drop for UploadInbox {
    fn drop(&mut self) {
        let _ = self.dispose();
    }
}
