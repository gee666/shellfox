use crate::SessionCreationFactMap;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use shellfox_core::*;
use std::{collections::HashSet, path::Path, time::Duration};

pub const STORE_FILENAME: &str = "shellfox-rust-v1.sqlite3";
const APPLICATION_ID: i32 = 0x53465852;
const STORE_VERSION: i32 = 5;
pub(crate) use crate::storage_write::save;

pub(crate) fn storage_error(_: impl std::fmt::Display) -> Error {
    Error::new(
        ErrorCode::StorageFailed,
        "Shellfox metadata storage failed.",
    )
}
pub(crate) fn open(path: &Path) -> Result<Connection> {
    let mut db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(storage_error)?;
    crate::approval_store::configure(&db)?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(storage_error)?;
    db.pragma_update(None, "foreign_keys", true)
        .map_err(storage_error)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    let version: i32 = tx
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage_error)?;
    let identity: i32 = tx
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(storage_error)?;
    if version > STORE_VERSION {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Metadata was created by a newer Shellfox host.",
        ));
    }
    if version == 0 {
        let tables: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |r| r.get(0)).map_err(storage_error)?;
        if identity != 0 || tables != 0 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "This is not an empty experiment store.",
            ));
        }
        tx.execute_batch("
            CREATE TABLE host_state (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
            CREATE TABLE sessions (id TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE tabs (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, ordinal INTEGER NOT NULL, value TEXT NOT NULL, UNIQUE(session_id,ordinal));
            CREATE TABLE requests (request_id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE);
        ").map_err(storage_error)?;
        tx.execute(
            "INSERT INTO host_state VALUES(1,?)",
            [encode(&State::default())?],
        )
        .map_err(storage_error)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(storage_error)?;
    } else if identity != APPLICATION_ID {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "This is not a Shellfox Rust experiment store.",
        ));
    }
    if version < 3 {
        crate::session_facts::migrate(&tx)?;
    }
    if version < 4 {
        crate::reserved_publication::migrate(&tx)?;
    }
    if version < 2 {
        crate::settings_store::migrate(&tx, version != 0)?;
    }
    if version < 5 {
        crate::approval_store::migrate(&tx)?;
    }
    if version < STORE_VERSION {
        tx.pragma_update(None, "user_version", STORE_VERSION)
            .map_err(storage_error)?;
        // A corrupt old store must roll back the whole migration, not gain an
        // empty facts table or silently default unrelated missing settings.
        load(&tx)?;
    }
    tx.commit().map_err(storage_error)?;
    db.pragma_update(None, "journal_mode", "WAL")
        .map_err(storage_error)?;
    Ok(db)
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct State {
    pub revision: u64,
    #[serde(
        serialize_with = "crate::settings_local::serialize",
        deserialize_with = "crate::settings_local::deserialize"
    )]
    pub preferences: Preferences,
    pub selection: Selection,
    #[serde(default, skip_serializing)]
    pub terminal_env: Vec<EnvVar>,
    pub last_change: Option<ChangedEvent>,
}
pub(crate) struct Data {
    pub state: State,
    pub sessions: Vec<Session>,
    pub requests: Vec<(SessionId, SessionId)>,
    pub creation_facts: SessionCreationFactMap,
}
impl Data {
    pub fn validate(&self) -> Result<()> {
        self.state.preferences.validate()?;
        validate_env(&self.state.terminal_env, cfg!(windows))?;
        let mut ids = HashSet::new();
        let mut tabs = HashSet::new();
        for session in &self.sessions {
            session.validate()?;
            validate_env(&session.env, cfg!(windows))?;
            validation::ensure(ids.insert(session.id), "Duplicate session ID.")?;
            for tab in &session.tabs {
                validation::ensure(tabs.insert(tab.id), "Duplicate tab ID.")?;
            }
        }
        for (id, facts) in &self.creation_facts {
            validation::ensure(
                ids.contains(id),
                "Session creation facts have no parent session.",
            )?;
            facts.validate()?;
        }
        let selection = &self.state.selection;
        if let Some(id) = selection.session_id {
            let session = self.session(id)?;
            if let Some(tab) = selection.tab_id {
                validation::ensure(
                    session.tabs.iter().any(|t| t.id == tab),
                    "Invalid selected tab.",
                )?;
            }
        } else {
            validation::ensure(
                selection.tab_id.is_none(),
                "Selected tab requires a session.",
            )?;
        }
        let mut requests = HashSet::new();
        for (request, session) in &self.requests {
            validation::ensure(
                requests.insert(request) && ids.contains(session),
                "Invalid create request reference.",
            )?;
        }
        Ok(())
    }
    pub fn session(&self, id: SessionId) -> Result<&Session> {
        self.sessions
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(not_found)
    }
    pub fn session_mut(&mut self, id: SessionId) -> Result<&mut Session> {
        self.sessions
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(not_found)
    }
    pub fn tab_mut(&mut self, id: TabId) -> Result<&mut Tab> {
        self.sessions
            .iter_mut()
            .flat_map(|s| &mut s.tabs)
            .find(|t| t.id == id)
            .ok_or_else(not_found)
    }
    pub fn tab(&self, id: TabId) -> Result<&Tab> {
        self.sessions
            .iter()
            .flat_map(|s| &s.tabs)
            .find(|t| t.id == id)
            .ok_or_else(not_found)
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut sessions = self.sessions.clone();
        sessions.sort_by(|a, b| {
            b.pinned_at
                .is_some()
                .cmp(&a.pinned_at.is_some())
                .then_with(|| match (&a.pinned_at, &b.pinned_at) {
                    (Some(a), Some(b)) => a.cmp(b),
                    _ => std::cmp::Ordering::Equal,
                })
                .then_with(|| a.settled_at.is_some().cmp(&b.settled_at.is_some()))
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        Snapshot {
            revision: self.state.revision,
            sessions,
            preferences: self.state.preferences.clone(),
            selection: self.state.selection.clone(),
        }
    }
    pub fn select(&mut self, session_id: SessionId, tab_id: Option<TabId>) {
        self.state.selection = Selection {
            session_id: Some(session_id),
            tab_id,
        };
    }
    pub fn repair_selection(&mut self) {
        let selected = self.state.selection.session_id;
        let session = self
            .sessions
            .iter()
            .find(|s| Some(s.id) == selected)
            .or_else(|| self.sessions.first());
        self.state.selection = match session {
            Some(s) => Selection {
                session_id: Some(s.id),
                tab_id: s
                    .tabs
                    .iter()
                    .find(|t| Some(t.id) == self.state.selection.tab_id)
                    .or_else(|| s.tabs.first())
                    .map(|t| t.id),
            },
            None => Selection::default(),
        };
    }
}
pub(crate) fn not_found() -> Error {
    Error::new(ErrorCode::NotFound, "Session or tab was not found.")
}
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(storage_error)
}
fn decode<T: serde::de::DeserializeOwned>(value: String) -> Result<T> {
    if value.len() > 16 * 1024 * 1024 {
        return Err(storage_error("oversized"));
    }
    serde_json::from_str(&value)
        .map_err(|_| Error::new(ErrorCode::StorageFailed, "Saved metadata is invalid."))
}
pub(crate) fn read(db: &Connection) -> Result<Data> {
    let tx = db.unchecked_transaction().map_err(storage_error)?;
    let data = load(&tx)?;
    tx.commit().map_err(storage_error)?;
    Ok(data)
}
pub(crate) fn load(db: &Connection) -> Result<Data> {
    let state = decode(
        db.query_row("SELECT value FROM host_state WHERE id=1", [], |r| r.get(0))
            .map_err(storage_error)?,
    )?;
    let mut sessions = Vec::<Session>::new();
    let mut stmt = db
        .prepare("SELECT id,value FROM sessions ORDER BY id")
        .map_err(storage_error)?;
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage_error)?
    {
        let (id, value) = row.map_err(storage_error)?;
        let session: Session = decode(value)?;
        validation::ensure(
            session.id.to_string() == id && session.tabs.is_empty(),
            "Invalid saved session identity.",
        )?;
        sessions.push(session);
    }
    let mut stmt = db
        .prepare("SELECT id,session_id,ordinal,value FROM tabs ORDER BY session_id,ordinal")
        .map_err(storage_error)?;
    for row in stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(storage_error)?
    {
        let (id, parent, ordinal, value) = row.map_err(storage_error)?;
        let tab: Tab = decode(value)?;
        validation::ensure(
            tab.id.to_string() == id
                && tab.session_id.to_string() == parent
                && tab.ordinal == ordinal,
            "Invalid saved tab identity.",
        )?;
        sessions
            .iter_mut()
            .find(|s| s.id == tab.session_id)
            .ok_or_else(not_found)?
            .tabs
            .push(tab);
    }
    let mut requests = Vec::new();
    let mut stmt = db
        .prepare("SELECT request_id,session_id FROM requests")
        .map_err(storage_error)?;
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage_error)?
    {
        let (a, b) = row.map_err(storage_error)?;
        requests.push((a.parse()?, b.parse()?));
    }
    let version: i32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage_error)?;
    let creation_facts = if version >= 3 {
        crate::session_facts::read(db)?
    } else {
        SessionCreationFactMap::new()
    };
    let mut data = Data {
        state,
        sessions,
        requests,
        creation_facts,
    };
    if version >= 2 {
        crate::settings_store::project(&crate::settings_store::read(db)?, &mut data)?;
    }
    data.validate()
        .map_err(|_| storage_error("invalid persisted state"))?;
    if version >= 4 {
        crate::reserved_publication::validate_store(db, &data)?;
    }
    Ok(data)
}
