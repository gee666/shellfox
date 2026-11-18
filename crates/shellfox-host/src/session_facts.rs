//! Historical session intent, not executable authority or a live process claim.
use crate::{storage, *};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionCreationFacts {
    pub version: u8,
    pub adapter_id: SettingsAdapter,
    pub shell_id: SettingsShell,
}
impl SessionCreationFacts {
    pub fn new(adapter_id: SettingsAdapter, shell_id: SettingsShell) -> Self {
        Self {
            version: 1,
            adapter_id,
            shell_id,
        }
    }
    pub fn validate(&self) -> Result<()> {
        validation::ensure(
            self.version == 1,
            "Unsupported session creation fact version.",
        )
    }
}
pub type SessionCreationFactMap = BTreeMap<SessionId, SessionCreationFacts>;
impl Host {
    /// Absent entries are UNKNOWN. Never fill them from current settings, an
    /// arbitrary tab, runtime identity, or a later shell/profile selection.
    pub fn session_creation_facts(&self) -> Result<SessionCreationFactMap> {
        let inner = self.lock()?;
        Ok(storage::read(&inner.db)?.creation_facts)
    }
}
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE session_creation_facts (
        session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
        value TEXT NOT NULL
    );",
    )
    .map_err(storage::storage_error)
}
pub(crate) fn read(db: &Connection) -> Result<SessionCreationFactMap> {
    let mut facts = SessionCreationFactMap::new();
    let mut statement = db
        .prepare("SELECT session_id,value FROM session_creation_facts ORDER BY session_id")
        .map_err(storage::storage_error)?;
    for row in statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage::storage_error)?
    {
        let (id, raw) = row.map_err(storage::storage_error)?;
        let key: SessionId = id.parse().map_err(storage::storage_error)?;
        if raw.len() > 4096 || key.to_string() != id {
            return Err(storage::storage_error("invalid session creation facts"));
        }
        let fact: SessionCreationFacts =
            serde_json::from_str(&raw).map_err(storage::storage_error)?;
        fact.validate().map_err(storage::storage_error)?;
        facts.insert(key, fact);
    }
    Ok(facts)
}
pub(crate) fn save(db: &Connection, facts: &SessionCreationFactMap) -> Result<()> {
    for (id, fact) in facts {
        fact.validate()?;
        db.execute(
            "INSERT INTO session_creation_facts VALUES(?,?)",
            rusqlite::params![id.to_string(), storage::encode(fact)?],
        )
        .map_err(storage::storage_error)?;
    }
    Ok(())
}
