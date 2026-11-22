//! Read-only import of the Electron repository schema, versions 1 through 5.
//!
//! Only validated session metadata and user settings cross this boundary.
//! Tabs are deliberately omitted, matching `src/main/terminal/service.ts::initialize`,
//! which deletes every saved tab because saved PIDs cannot restore app-owned handles.
//! Operations, terminal registrations, commands, credentials, output and window
//! bindings are never selected. Saved environment requires explicit import consent.

use std::{collections::HashSet, path::Path, time::Duration};

use crate::{SessionCreationFactMap, SessionCreationFacts, SettingsImportPolicy};
use rusqlite::{Connection, OpenFlags};
use shellfox_core::{Error, ErrorCode, Result, Session, Settings, SettingsAdapter, SettingsShell};

pub(crate) struct LegacyImport {
    pub sessions: Vec<Session>,
    pub settings: Settings,
    pub creation_facts: SessionCreationFactMap,
}

pub(crate) fn read_legacy(path: &Path, policy: SettingsImportPolicy) -> Result<LegacyImport> {
    // The caller preflights this absolute path and its sidecars without following links.
    let mut db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| storage())?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(|_| storage())?;
    // These connection-local settings do not alter the source schema or data.
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(|_| storage())?;
    let transaction = db.transaction().map_err(|_| storage())?;
    let version: i64 = transaction
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|_| storage())?;
    if !(1..=5).contains(&version) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Unsupported legacy database version",
        ));
    }
    validate_schema(&transaction, version)?;
    let integrity: String = transaction
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|_| storage())?;
    if integrity != "ok" {
        return Err(storage());
    }
    let settings = crate::settings_legacy::read(&transaction, policy)?;
    let (sessions, creation_facts) = read_sessions(&transaction, version, policy)?;
    // A deferred read transaction gives settings and sessions one consistent snapshot.
    transaction.commit().map_err(|_| storage())?;
    Ok(LegacyImport {
        sessions,
        settings,
        creation_facts,
    })
}

fn validate_schema(db: &Connection, version: i64) -> Result<()> {
    let executable_schema: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type IN ('view','trigger') \
             OR upper(sql) LIKE 'CREATE VIRTUAL TABLE%'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| storage())?;
    if executable_schema != 0 {
        return Err(invalid());
    }
    let mut sessions = String::from(
        "id TEXT,title TEXT,cwd TEXT,adapterId TEXT,shellId TEXT,shellExecutable TEXT,\
         createdAt TEXT,updatedAt TEXT,settledAt TEXT,error TEXT,target TEXT",
    );
    if version >= 3 {
        sessions.push_str(",env TEXT");
    }
    if version >= 4 {
        sessions.push_str(",pinnedAt TEXT");
    }
    validate_table(db, "sessions", &sessions)?;
    validate_table(db, "tabs", "id TEXT,sessionId TEXT,title TEXT,cwd TEXT,ordinal INTEGER,createdAt TEXT,lifecycle TEXT,operationId TEXT,registration TEXT,error TEXT")?;
    validate_table(db, "operations", "id TEXT,sessionId TEXT,tabId TEXT,requestId TEXT,kind TEXT,state TEXT,createdAt TEXT,updatedAt TEXT,error TEXT")?;
    validate_table(db, "settings", "id INTEGER,value TEXT")?;
    validate_table(db, "preferences", "key TEXT,value TEXT")?;
    if version >= 2 {
        validate_table(db, "terminal_metadata", "tabId TEXT,value TEXT")?;
    } else {
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='terminal_metadata'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| storage())?;
        if count != 0 {
            return Err(invalid());
        }
    }
    Ok(())
}

fn validate_table(db: &Connection, table: &str, expected: &str) -> Result<()> {
    let kind: Option<String> = db
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name=?1",
            [table],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(|_| storage())?;
    if kind.as_deref() != Some("table") {
        return Err(invalid());
    }
    // Names come only from the constants above, never from database content or input.
    let mut statement = db
        .prepare(&format!("PRAGMA table_xinfo('{table}')"))
        .map_err(|_| storage())?;
    let columns = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|_| storage())?;
    let mut expected = expected.split(',');
    for (index, column) in columns.enumerate() {
        let (name, kind, primary, hidden) = column.map_err(|_| storage())?;
        let Some(specification) = expected.next() else {
            return Err(invalid());
        };
        let Some((expected_name, expected_kind)) = specification.trim().split_once(' ') else {
            return Err(invalid());
        };
        if name != expected_name
            || !kind.eq_ignore_ascii_case(expected_kind)
            || primary != i64::from(index == 0)
            || hidden != 0
        {
            return Err(invalid());
        }
    }
    if expected.next().is_some() {
        return Err(invalid());
    }
    Ok(())
}

fn read_sessions(
    db: &Connection,
    version: i64,
    policy: SettingsImportPolicy,
) -> Result<(Vec<Session>, SessionCreationFactMap)> {
    let pin = if version >= 4 { "pinnedAt" } else { "NULL" };
    let env = if version >= 3 && policy == SettingsImportPolicy::IncludeEnvironment {
        "CAST(env AS BLOB)"
    } else {
        "NULL"
    };
    let mut statement = db.prepare(&format!(
        "SELECT id,title,cwd,createdAt,updatedAt,settledAt,{pin},adapterId,shellId,{env} FROM sessions ORDER BY id LIMIT 10001"
    )).map_err(|_| storage())?;
    let mut rows = statement.query([]).map_err(|_| storage())?;
    let mut sessions = Vec::new();
    let mut creation_facts = SessionCreationFactMap::new();
    let mut ids = HashSet::new();
    while let Some(row) = rows.next().map_err(|_| storage())? {
        if sessions.len() == 10000 {
            return Err(invalid());
        }
        let id: String = row.get(0).map_err(|_| storage())?;
        let session = Session {
            id: id.parse()?,
            title: row.get(1).map_err(|_| storage())?,
            cwd: row.get(2).map_err(|_| storage())?,
            created_at: row.get(3).map_err(|_| storage())?,
            updated_at: row.get(4).map_err(|_| storage())?,
            settled_at: row.get(5).map_err(|_| storage())?,
            pinned_at: row.get(6).map_err(|_| storage())?,
            tabs: Vec::new(),
            env: match row.get::<_, Option<Vec<u8>>>(9).map_err(|_| invalid())? {
                Some(bytes) if bytes.len() <= 8 * 1024 * 1024 => {
                    serde_json::from_slice(&bytes).map_err(|_| invalid())?
                }
                Some(_) => return Err(invalid()),
                None => Vec::new(),
            },
        };
        session.validate()?;
        if !ids.insert(session.id) {
            return Err(invalid());
        }
        // These are the exact stored creation columns, not current settings or
        // a saved runtime/registration. Invalid present columns fail the import.
        let adapter: SettingsAdapter = serde_json::from_value(serde_json::Value::String(
            row.get(7).map_err(|_| invalid())?,
        ))
        .map_err(|_| invalid())?;
        let shell: SettingsShell = serde_json::from_value(serde_json::Value::String(
            row.get(8).map_err(|_| invalid())?,
        ))
        .map_err(|_| invalid())?;
        creation_facts.insert(session.id, SessionCreationFacts::new(adapter, shell));
        sessions.push(session);
    }
    Ok((sessions, creation_facts))
}

fn invalid() -> Error {
    Error::new(ErrorCode::Validation, "Invalid legacy database metadata")
}

fn storage() -> Error {
    // SQLite diagnostics can contain source paths and stored values. Keep them private.
    Error::new(ErrorCode::StorageFailed, "Could not read legacy database")
}
