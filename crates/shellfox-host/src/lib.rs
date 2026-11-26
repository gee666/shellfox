//! Headless Shellfox metadata host. Local terminal ownership belongs to its adapter.
mod activity_store;
mod approval_store;
mod boundary;
#[path = "window_state.rs"]
mod desktop_window_state;
mod electron_merge;
mod journal;
mod legacy;
mod lifecycle;
mod reserved_publication;
mod session_creation;
mod session_facts;
mod sessions;
mod settings;
mod settings_legacy;
mod settings_local;
mod settings_profiles;
mod settings_store;
mod storage;
mod storage_guard;
mod storage_write;
mod tabs;
#[cfg(feature = "test-support")]
mod test_support;

pub use activity_store::{
    activity_unix_ms, activity_utc_timestamp, CapturedSettlement, ResumeSessionResult,
};
pub use boundary::HostRequest;
pub use electron_merge::{ElectronMerge, ElectronSettingsDigest};
use journal::{Journal, Write};
pub use journal::{JournalHook, JournalHooks};
pub use reserved_publication::{ReservedPublicationReceipt, ReservedSessionPublication};
use rusqlite::Connection;
pub use session_creation::CreationRequestReceipt;
pub use session_facts::{SessionCreationFactMap, SessionCreationFacts};
pub use settings::{SettingsRequest, SettingsRuntime};
pub use settings_legacy::{ElectronImport, SettingsImportPolicy};
pub use settings_profiles::{normalize_settings_profile, SettingsProfile};
pub use shellfox_core::*;
use std::{
    collections::HashMap,
    fs::File,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
};
pub use storage::STORE_FILENAME;
use storage::{storage_error, Data};

pub struct Host {
    inner: Mutex<Inner>,
    /// Last committed metadata revision. Written only by `Host::commit`.
    revision: AtomicU64,
    _ownership: File,
}
struct Inner {
    /// The in-memory working copy. Reads never touch the disk; writes reach it
    /// through `journal`.
    db: Connection,
    journal: Journal,
    registry: Registry,
    settings_runtime: Option<std::sync::Arc<dyn SettingsRuntime>>,
}
#[derive(Default)]
struct Registry {
    live: HashMap<TabId, Generation>,
    pending: HashMap<CleanupToken, PendingClose>,
    last_settlement_ms: u64,
}
#[derive(Clone)]
enum CloseTarget {
    Tab(TabId),
    Session(SessionId),
}
struct PendingClose {
    target: CloseTarget,
    revision: u64,
    terminals: Vec<TerminalIdentity>,
}
/// A child forked by any thread holds a copy of the lock until it execs, so a
/// store its host just released can look owned for a moment. A live host keeps
/// the lock for good.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
fn lock_store(ownership: &File) -> Result<()> {
    let deadline = std::time::Instant::now() + LOCK_WAIT;
    loop {
        match ownership.try_lock() {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Another host owns this experiment store.",
                ))
            }
        }
    }
}
impl Host {
    /// Opens only this explicit directory's separately named experiment store.
    /// One host owns the directory. Share an Arc<Host> between local clients.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let (directory, ownership, _checked_artifacts) =
            storage_guard::prepare(directory.as_ref())?;
        lock_store(&ownership)?;
        let disk = storage::open(&directory.join(STORE_FILENAME))?;
        let db = storage::working_copy(&disk)?;
        let journal = Journal::new(disk, &db)?;
        let host = Self {
            inner: Mutex::new(Inner {
                db,
                journal,
                registry: Registry::default(),
                settings_runtime: None,
            }),
            revision: AtomicU64::new(0),
            _ownership: ownership,
        };
        let snapshot = host.snapshot()?;
        host.revision.store(snapshot.revision, Ordering::Release);
        host.lock()?.registry.last_settlement_ms =
            activity_store::latest_settlement_ms(&snapshot.sessions)?;
        Ok(host)
    }
    /// Last committed metadata revision (sessions, tabs, settings), without SQL.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    /// Commits a metadata write, then publishes `revision`. A failed commit
    /// publishes nothing, so readers never see a rolled-back revision.
    fn commit(&self, tx: Write<'_>, revision: u64) -> Result<()> {
        tx.commit()?;
        self.revision.fetch_max(revision, Ordering::Release);
        Ok(())
    }
    /// From now on writes commit in memory and reach the disk in order on a
    /// writer thread: a slow disk delays only `durable()`, never a caller.
    /// Before this, each write waits for its disk commit and fails with it.
    pub fn start_journal(&self, hooks: JournalHooks) -> Result<()> {
        self.lock()?.journal.start(hooks)
    }
    /// Sequence number of the last committed change that changed a row.
    pub fn written(&self) -> Result<u64> {
        Ok(self.lock()?.journal.written())
    }
    /// Sequence number of the last change committed on disk.
    pub fn durable(&self) -> Result<u64> {
        Ok(self.lock()?.journal.durable())
    }
    /// The disk error the journal is retrying, while it fails.
    pub fn durability_failure(&self) -> Result<Option<String>> {
        Ok(self.lock()?.journal.failure())
    }
    /// Waits at most `timeout` until every committed change is on disk.
    pub fn flush(&self, timeout: std::time::Duration) -> Result<()> {
        self.lock()?.journal.flush(timeout)
    }
    fn lock(&self) -> Result<MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Host state is unavailable."))
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let mut inner = self.lock()?;
        let tx = inner.db.transaction().map_err(storage_error)?;
        let snapshot = storage::load(&tx)?.snapshot();
        tx.commit().map_err(storage_error)?;
        Ok(snapshot)
    }
    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        Ok(self.snapshot()?.sessions)
    }
    pub fn preferences(&self) -> Result<Preferences> {
        Ok(self.snapshot()?.preferences)
    }
    pub fn update_preferences(&self, patch: PreferencesPatch) -> Result<Snapshot> {
        self.update(ChangeReason::Settings, move |data, _| {
            let prefs = data.state.preferences.patched(patch)?;
            let changed = prefs != data.state.preferences;
            data.state.preferences = prefs;
            Ok(changed)
        })
    }
    /// Current revision notification. Clients that miss revisions fetch snapshot.
    pub fn changed_since(&self, revision: u64) -> Result<Option<ChangedEvent>> {
        let inner = self.lock()?;
        let data = storage::read(&inner.db)?;
        Ok(data
            .state
            .last_change
            .filter(|event| event.revision > revision))
    }
    pub fn terminal_launch(&self, tab_id: TabId) -> Result<TerminalLaunch> {
        let inner = self.lock()?;
        let data = storage::read(&inner.db)?;
        let tab = data.tab(tab_id)?;
        let session = data.session(tab.session_id)?;
        let mut env = data.state.terminal_env.clone();
        for entry in &session.env {
            env.retain(|old| {
                if cfg!(windows) {
                    !old.name.eq_ignore_ascii_case(&entry.name)
                } else {
                    old.name != entry.name
                }
            });
            env.push(entry.clone());
        }
        Ok(TerminalLaunch {
            session_id: session.id,
            tab_id,
            profile_id: tab.profile_id.clone(),
            wsl_launch: tab.wsl_launch.clone(),
            cwd: tab.cwd.clone(),
            env,
        })
    }
    pub fn set_session_env(&self, id: SessionId, env: Vec<EnvVar>) -> Result<Snapshot> {
        let env = normalize_env(env, cfg!(windows))?;
        self.update(ChangeReason::Sessions, move |data, _| {
            let session = data.session_mut(id)?;
            let changed = session.env != env;
            session.env = env;
            Ok(changed)
        })
    }
    pub fn set_terminal_env(&self, env: Vec<EnvVar>) -> Result<Snapshot> {
        let env = normalize_env(env, cfg!(windows))?;
        self.update(ChangeReason::Settings, move |data, _| {
            let changed = data.state.terminal_env != env;
            data.state.terminal_env = env;
            Ok(changed)
        })
    }
    /// Import into an empty experiment store only. The source is never opened for writing.
    pub fn import_legacy(&self, path: impl AsRef<Path>) -> Result<Snapshot> {
        self.import_legacy_with_settings(path, SettingsImportPolicy::ExcludeEnvironment)
    }
    fn update(
        &self,
        reason: ChangeReason,
        action: impl FnOnce(&mut Data, &Registry) -> Result<bool>,
    ) -> Result<Snapshot> {
        let mut inner = self.lock()?;
        self.update_locked(&mut inner, reason, action)
    }
    fn update_locked(
        &self,
        inner: &mut Inner,
        reason: ChangeReason,
        action: impl FnOnce(&mut Data, &Registry) -> Result<bool>,
    ) -> Result<Snapshot> {
        let Inner {
            db,
            journal,
            registry,
            settings_runtime,
        } = inner;
        let tx = Write::begin(db, journal)?;
        let mut data = storage::load(&tx)?;
        let old_preferences = data.state.preferences.clone();
        let old_env = data.state.terminal_env.clone();
        if action(&mut data, registry)? {
            settings_store::merge_compatibility(&tx, &old_preferences, &old_env, &mut data)?;
            data.state.revision = data
                .state
                .revision
                .checked_add(1)
                .ok_or_else(|| storage_error("revision overflow"))?;
            data.state.last_change = Some(ChangedEvent {
                revision: data.state.revision,
                reason,
                select_session_id: data.state.selection.session_id,
            });
            storage::save(&tx, &data)?;
        }
        let snapshot = data.snapshot();
        if reason == ChangeReason::Settings {
            if let Some(runtime) = settings_runtime.as_ref() {
                runtime.check_commit()?;
            }
        }
        self.commit(tx, snapshot.revision)?;
        Ok(snapshot)
    }
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
