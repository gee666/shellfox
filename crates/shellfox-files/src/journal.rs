//! V2 tracks intent, not serialized cleanup authority. Restart preserves every prior user stage.
use crate::{content, paths, user_stage_custody::UserStageCustody, *};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
pub(crate) const JOURNAL_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const JOURNAL_ENTRIES: usize = 32;
pub(crate) type SharedJournal = Arc<Mutex<StagingJournal>>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct JournalEntry {
    pub stage: String,
    pub target: String,
    pub phase: StagePhase,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StagePhase {
    Intent,
    Owned,
    Publishing,
    PreservedUnknown,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JournalDocument {
    pub version: u8,
    pub entries: Vec<JournalEntry>,
}
pub(crate) struct StagingJournal {
    pub root: crate::service::journal_anchor::JournalRoot,
    pub entries: BTreeMap<String, JournalEntry>,
    pub(crate) expected: Option<content::Snapshot>,
    pub(crate) live: BTreeMap<String, Arc<UserStageCustody>>,
    pub(crate) metadata_stage: Option<crate::journal_storage::MetadataStage>,
    pub(crate) cleanup_error: Option<FileError>,
    _lease: cap_std::fs::File,
}
impl StagingJournal {
    pub fn open(directory: &Path) -> Result<Self> {
        paths::validate_absolute_syntax(directory)?;
        let issuer = Arc::new(FileService::local_desktop(Access::ReadWrite));
        let plan = issuer.plan_directory(&Location {
            path: paths::display(directory)?,
        })?;
        let authority = match plan.create_private_approved_root() {
            Ok(root) => root,
            Err(mut failure) => {
                failure.preserve_stale().map_err(|_| invalid())?;
                return Err(FileError::new(failure.error().code,&failure.error().message));
            }
        };
        Self::open_anchored(authority)
    }
    pub fn open_anchored(authority: Arc<FileService>) -> Result<Self> {
        let _mutation = FileService::mutation_lock()?;
        let root = crate::service::journal_anchor::JournalRoot::open_verified(authority)?;
        // Separate V2 lease; never inspect, rewrite, remove or adopt V1/legacy .next artifacts.
        let lease = crate::journal_lease::acquire_v2(&root)?;
        let target = root.target("file-transfer-stages-v2.json")?;
        let (entries, expected) = match content::open_regular(&target.parent, &target.name) {
            Ok(mut file) => {
                crate::upload_private::require_owner(&file, &target.parent)?;
                let (bytes, snapshot) = content::read(&mut file, JOURNAL_BYTES)?;
                let document: JournalDocument =
                    serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                if document.version != 2 || document.entries.len() > JOURNAL_ENTRIES {
                    return Err(invalid());
                }
                (document.entries, Some(snapshot))
            }
            Err(error) if error.code == ErrorCode::NotFound => (Vec::new(), None),
            Err(error) => return Err(error),
        };
        let mut result = Self {
            root,
            entries: BTreeMap::new(),
            expected,
            live: BTreeMap::new(),
            metadata_stage: None,
            cleanup_error: None,
            _lease: lease,
        };
        for mut entry in entries {
            if !stage_name(Path::new(&entry.stage))
                || entry.target.len() > 32767
                || result.entries.contains_key(&entry.stage)
            {
                return Err(invalid());
            }
            // No authority/receipt survives a process restart. Do NOT open recorded user paths.
            entry.phase = StagePhase::PreservedUnknown;
            result.entries.insert(entry.stage.clone(), entry);
        }
        if result.expected.is_none() {result.persist(&_mutation)?;}
        Ok(result)
    }
    pub fn reserve_stage(
        &mut self,
        capability: Arc<FileService>,
        selector: &str,
        target: &paths::Target,
        name: &Path,
        mutation: &std::sync::MutexGuard<'static, ()>,
    ) -> Result<Arc<UserStageCustody>> {
        self.root.check()?;
        if self.entries.len() >= JOURNAL_ENTRIES {
            return Err(invalid());
        }
        let stage = paths::display(&target.absolute.with_file_name(name))?;
        if !stage_name(Path::new(&stage)) || self.entries.contains_key(&stage) {
            return Err(invalid());
        }
        let custody = UserStageCustody::reserve(capability, selector, target, name)?;
        self.entries.insert(
            stage.clone(),
            JournalEntry {
                stage: stage.clone(),
                target: selector.into(),
                phase: StagePhase::Intent,
            },
        );
        self.live.insert(stage, custody.clone()); // original owner installed BEFORE persist/native effect
        self.persist(mutation)?;
        Ok(custody)
    }
    pub fn phase(
        &mut self,
        stage: &Path,
        phase: StagePhase,
        mutation: &std::sync::MutexGuard<'static, ()>,
    ) -> Result<()> {
        self.root.check()?;
        let entry = self
            .entries
            .get_mut(&paths::display(stage)?)
            .ok_or_else(invalid)?;
        entry.phase = phase;
        self.persist(mutation)
    }
    pub fn finish_stage(
        &mut self,
        stage: &Path,
        removed_or_published: bool,
        mutation: &std::sync::MutexGuard<'static, ()>,
    ) -> Result<()> {
        let key = paths::display(stage)?;
        if removed_or_published {
            self.entries.remove(&key);
        } else if let Some(entry) = self.entries.get_mut(&key) {
            entry.phase = StagePhase::PreservedUnknown;
        }
        self.persist(mutation)?;
        self.live.remove(&key);
        Ok(())
    }
}
pub(crate) fn stage_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix(".shellfox-transfer-"))
        .and_then(|s| s.strip_suffix(".partial"))
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id))
}
pub(crate) fn invalid() -> FileError {
    FileError::new(
        ErrorCode::StorageFailed,
        "Invalid, full or unavailable V2 transfer journal; retained stages were not deleted.",
    )
}
impl TransferRegistry {
    pub fn initialize_journal(&mut self, directory: impl AsRef<Path>) -> Result<()> {
        if self.disposed || !self.entries.is_empty() || self.journal.is_some() {
            return Err(invalid());
        }
        self.journal = Some(Arc::new(Mutex::new(StagingJournal::open(
            directory.as_ref(),
        )?)));
        self.initialized = true;
        Ok(())
    }
    pub fn initialize_journal_anchored(&mut self, journal: Arc<FileService>) -> Result<()> {
        if self.disposed || !self.entries.is_empty() || self.journal.is_some() {
            return Err(invalid());
        }
        self.journal = Some(Arc::new(Mutex::new(StagingJournal::open_anchored(
            journal,
        )?)));
        self.initialized = true;
        Ok(())
    }
}
