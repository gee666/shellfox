//! The Host repository is the only approval writer. Legacy files are inspection only.
use crate::{error, Approval, ApprovalRepository, ViewPermission};
use shellfox_core::{ApprovalStorePolicy, ApprovalStoreRecord, ApprovalStoreSnapshot};
use shellfox_files::{ErrorCode as FileCode, FileService};
use shellfox_protocol::{AppError, CancellationToken, ErrorCode};
use std::{
    ffi::OsStr,
    sync::{Arc, Mutex},
};
const LIMIT: usize = 1024 * 1024;
struct Preparation {
    directory: Option<Arc<FileService>>,
    /// A failed open is not retried; the store stays unavailable.
    failed: bool,
}
#[derive(Default)]
struct RepositoryState {
    repository: Option<Arc<dyn ApprovalRepository>>,
    revision: Option<u64>,
    uncertain: bool,
}
/// Retain before initialization. Files authority is only for legacy inspection.
pub struct PrivateApprovalStore {
    control: Arc<FileService>,
    lost: CancellationToken,
    preparation: Mutex<Preparation>,
    repository: Mutex<RepositoryState>,
    gate: tokio::sync::Mutex<()>,
}
fn failed() -> AppError {
    error(
        ErrorCode::StorageFailed,
        "Private approval metadata unavailable; no recovery or mutation was retried.",
        false,
    )
}
impl PrivateApprovalStore {
    pub fn retained_native(control: Arc<FileService>, lost: CancellationToken) -> Self {
        Self {
            control,
            lost,
            preparation: Mutex::new(Preparation {
                directory: None,
                failed: false,
            }),
            repository: Mutex::default(),
            gate: tokio::sync::Mutex::new(()),
        }
    }
    /// Bind once to the actual already-started Host actor, never a path fallback.
    pub fn bind_repository(&self, repository: Arc<dyn ApprovalRepository>) -> Result<(), AppError> {
        self.owned()?;
        let mut state = self.repository.lock().map_err(|_| failed())?;
        if state.repository.is_some() || state.uncertain {
            return Err(failed());
        }
        state.repository = Some(repository);
        Ok(())
    }
    fn owned(&self) -> Result<(), AppError> {
        if self.lost.is_cancelled() {
            Err(failed())
        } else {
            Ok(())
        }
    }
    pub fn initialize_read_only(&self) -> Result<(), AppError> {
        self.owned()?;
        let mut state = self.preparation.lock().map_err(|_| failed())?;
        if state.failed {
            return Err(failed());
        }
        if state.directory.is_none() {
            match self
                .control
                .private_child(OsStr::new("remote-v1"))
            {
                Ok(directory) => state.directory = Some(directory),
                Err(_) => {
                    state.failed = true;
                    return Err(failed());
                }
            }
        }
        self.owned()
    }
    fn directory(&self) -> Result<Arc<FileService>, AppError> {
        self.preparation
            .lock()
            .map_err(|_| failed())?
            .directory
            .clone()
            .ok_or_else(failed)
    }
    fn read(
        &self,
        directory: &FileService,
        leaf: &OsStr,
    ) -> Result<Option<shellfox_files::private::PrivateRead>, AppError> {
        self.owned()?;
        let result = directory.read_private(leaf, LIMIT);
        self.owned()?;
        match result {
            Ok(read) => Ok(Some(read)),
            Err(error) if error.code == FileCode::NotFound => Ok(None),
            Err(_) => Err(failed()),
        }
    }
    fn legacy_clear(&self) -> Result<(), AppError> {
        let directory = self.directory()?;
        if self
            .read(&directory, OsStr::new("write-pending"))?
            .is_some()
            || self
                .read(&directory, OsStr::new("approvals.json"))?
                .is_some()
        {
            return Err(error(
                ErrorCode::Unsupported,
                "Legacy approval metadata requires an explicit migration; it was preserved.",
                false,
            ));
        }
        Ok(())
    }
    /// Historical inventory only when no repository is bound. Never grant from it.
    pub async fn load(&self) -> Result<Vec<Approval>, AppError> {
        let bound = self
            .repository
            .lock()
            .map_err(|_| failed())?
            .repository
            .is_some();
        if bound {
            return self.load_policy().await;
        }
        let _gate = self.gate.lock().await;
        let directory = self.directory()?;
        if self
            .read(&directory, OsStr::new("write-pending"))?
            .is_some()
        {
            return Err(failed());
        }
        match self.read(&directory, OsStr::new("approvals.json"))? {
            Some(read) => crate::approval_records::decode(&read.bytes),
            None => Ok(Vec::new()),
        }
    }
    fn enter_repository(
        &self,
        require_revision: bool,
    ) -> Result<(Arc<dyn ApprovalRepository>, Option<u64>), AppError> {
        self.owned()?;
        let mut state = self.repository.lock().map_err(|_| failed())?;
        if state.uncertain || (require_revision && state.revision.is_none()) {
            return Err(failed());
        }
        let repository = state.repository.clone().ok_or_else(|| {
            error(
                ErrorCode::Unsupported,
                "The original Host approval repository is not bound.",
                false,
            )
        })?;
        // Installed before await/enqueue. Cancellation leaves this store unusable.
        state.uncertain = true;
        Ok((repository, state.revision))
    }
    fn finish_repository(&self, snapshot: &ApprovalStoreSnapshot) -> Result<(), AppError> {
        self.owned()?;
        snapshot.validate().map_err(|_| failed())?;
        let mut state = self.repository.lock().map_err(|_| failed())?;
        state.revision = Some(snapshot.revision);
        state.uncertain = false;
        Ok(())
    }
    pub(crate) async fn load_policy(&self) -> Result<Vec<Approval>, AppError> {
        let _gate = self.gate.lock().await;
        let (repository, _) = self.enter_repository(false)?;
        self.legacy_clear()?;
        let loaded = repository.load().await?;
        loaded.validate().map_err(|_| failed())?;
        let approvals: Vec<_> = loaded
            .approvals
            .iter()
            .map(|record| Approval {
                peer_id: record.peer_id.clone(),
                hostname: record.hostname.clone(),
                approved_at_millis: record.approved_at_millis,
                permission: ViewPermission::Write,
                file_roots: Vec::new(),
            })
            .collect();
        crate::approval_records::validate(&approvals)?;
        self.finish_repository(&loaded)?;
        Ok(approvals)
    }
    pub async fn save(&self, approvals: &[Approval]) -> Result<(), AppError> {
        let _gate = self.gate.lock().await;
        crate::approval_records::validate(approvals)?;
        if approvals.iter().any(|record| {
            record.permission != ViewPermission::Write || !record.file_roots.is_empty()
        }) {
            return Err(error(
                ErrorCode::Validation,
                "Approval writes require the fixed full-account policy.",
                false,
            ));
        }
        let (repository, revision) = self.enter_repository(true)?;
        self.legacy_clear()?;
        let records: Vec<_> = approvals
            .iter()
            .map(|approval| ApprovalStoreRecord {
                peer_id: approval.peer_id.clone(),
                hostname: approval.hostname.clone(),
                approved_at_millis: approval.approved_at_millis,
                policy: ApprovalStorePolicy::ControlFullAccountV1,
            })
            .collect();
        let revision = revision.ok_or_else(failed)?;
        let mut expected = ApprovalStoreSnapshot {
            revision: revision.checked_add(1).ok_or_else(failed)?,
            approvals: records.clone(),
        };
        expected.approvals.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
        let committed = repository.commit(revision, records).await?;
        if committed != expected {
            return Err(failed());
        }
        self.finish_repository(&committed)
    }
}
