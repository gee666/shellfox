use crate::{staged_write::StagedWrite, stream_io::Inspected, *};
use cap_std::fs::File;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;

pub struct TransferRegistry {
    pub(crate) limits: TransferLimits,
    pub(crate) entries: HashMap<Uuid, Transfer>,
    pub(crate) receipts: VecDeque<Receipt>,
    pub(crate) disposed: bool,
    pub(crate) staging_limit: u64,
}
pub(crate) struct Transfer {
    pub actor: TransferActor,
    pub capability: Arc<FileService>,
    pub touched: Instant,
    pub state: TransferState,
}
impl Transfer {
    fn asset(&self) -> bool {
        matches!(&self.state, TransferState::Read(read) if read.asset)
    }
}
pub(crate) struct TransferRead {
    pub file: File,
    pub inspected: Inspected,
    pub offset: u64,
    pub last: Option<(u64, ReadChunkResult)>,
    pub asset: bool,
}
pub(crate) enum TransferState {
    Read(Box<TransferRead>),
    Write(Box<StagedWrite>),
}
pub(crate) struct Receipt {
    pub id: Uuid,
    pub actor: TransferActor,
    pub capability: Arc<FileService>,
    pub touched: Instant,
    pub result: WriteResult,
    pub published: crate::HandleIdentity,
}
#[derive(Debug, Clone)]
pub struct TransferUsage {
    pub active: usize,
    pub staged_bytes: u64,
    pub receipts: usize,
}

impl TransferRegistry {
    pub fn new(limits: TransferLimits) -> Result<Self> {
        if limits.max_file_bytes == 0
            || limits.max_file_bytes > TRANSFER_MAX_BYTES
            || limits.idle_ttl.is_zero()
            || limits.idle_ttl > std::time::Duration::from_secs(300)
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid transfer limits.",
            ));
        }
        let staging_limit = limits.max_file_bytes * 2;
        Ok(Self {
            limits,
            staging_limit,
            entries: HashMap::new(),
            receipts: VecDeque::new(),
            disposed: false,
        })
    }
    pub fn usage(&self) -> TransferUsage {
        TransferUsage {
            active: self.entries.len(),
            staged_bytes: self
                .entries
                .values()
                .map(|entry| match &entry.state {
                    TransferState::Write(stage) => stage.input.size,
                    _ => 0,
                })
                .sum(),
            receipts: self.receipts.len(),
        }
    }
    pub(crate) fn check(
        &self,
        actor: &TransferActor,
        capability: &FileService,
        write: bool,
        auth: &dyn TransferAuthorization,
    ) -> Result<()> {
        if self.disposed {
            return Err(FileError::denied());
        }
        capability.authorized(write)?;
        auth.check(actor, capability, write)
    }
    pub(crate) fn reserve(&mut self, actor: &TransferActor, size: u64, asset: bool) -> Result<()> {
        // Request-scoped policy may know only this actor. Never revoke unrelated transfers.
        self.entries
            .retain(|_, entry| entry.touched.elapsed() <= self.limits.idle_ttl);
        self.receipts
            .retain(|entry| entry.touched.elapsed() <= self.limits.idle_ttl);
        if self.disposed {
            return Err(FileError::denied());
        }
        let (per_actor, total) = if asset {
            (ASSET_READS_PER_ACTOR, ASSET_READS_TOTAL)
        } else {
            (TRANSFERS_PER_ACTOR, TRANSFERS_TOTAL)
        };
        let same = || self.entries.values().filter(|entry| entry.asset() == asset);
        if same().filter(|entry| entry.actor.owner == actor.owner).count() >= per_actor
            || same().count() >= total
            || self.usage().staged_bytes + size > self.staging_limit
        {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Host transfer quota reached. Finish or cancel an existing transfer.",
            ));
        }
        Ok(())
    }
    pub(crate) fn take(&mut self, actor: &TransferActor, id: Uuid) -> Result<Transfer> {
        let found = self.entries.get(&id).ok_or_else(missing)?;
        if &found.actor != actor {
            return Err(missing());
        }
        let expired = found.touched.elapsed() > self.limits.idle_ttl;
        let entry = self.entries.remove(&id).ok_or_else(missing)?;
        if expired {
            return Err(missing());
        }
        Ok(entry)
    }
    /// An owner can discard its own partial even after losing approval or terminal control.
    pub fn cancel(&mut self, actor: &TransferActor, input: &TransferIdentity) -> Result<()> {
        if let Some(entry) = self.entries.get(&input.transfer_id) {
            if &entry.actor != actor {
                return Err(missing());
            }
            self.entries.remove(&input.transfer_id);
            return Ok(());
        }
        if let Some(index) = self
            .receipts
            .iter()
            .position(|entry| entry.id == input.transfer_id && &entry.actor == actor)
        {
            self.receipts.remove(index);
            return Ok(());
        }
        Err(missing())
    }
    pub fn cancel_actor(&mut self, owner: &str) {
        self.entries.retain(|_, entry| entry.actor.owner != owner);
        self.receipts.retain(|entry| entry.actor.owner != owner);
    }
    /// Host-wide policy provider must be able to validate every stored actor.
    pub fn sweep(&mut self, auth: &dyn TransferAuthorization) -> Result<()> {
        let ttl = self.limits.idle_ttl;
        self.entries.retain(|_, entry| {
            let write = matches!(entry.state, TransferState::Write(_));
            entry.touched.elapsed() <= ttl
                && entry.capability.authorized(write).is_ok()
                && auth.check(&entry.actor, &entry.capability, write).is_ok()
        });
        self.receipts.retain(|entry| {
            entry.touched.elapsed() <= ttl
                && entry.capability.authorized(true).is_ok()
                && auth.check(&entry.actor, &entry.capability, true).is_ok()
        });
        Ok(())
    }
    pub fn dispose(&mut self) {
        self.disposed = true;
        self.entries.clear();
        self.receipts.clear();
    }
}
pub(crate) fn missing() -> FileError {
    FileError::new(
        ErrorCode::NotFound,
        "Transfer not found for this connection generation.",
    )
}

impl Drop for TransferRegistry {
    fn drop(&mut self) {
        self.dispose();
    }
}
