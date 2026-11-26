use crate::{storage::storage_error, Host, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use shellfox_core::window_state::WindowState;
use std::path::Path;

const KEY: &str = "window-state";

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE desktop_preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        .map_err(storage_error)
}

impl Host {
    pub fn window_state(&self) -> Result<Option<WindowState>> {
        read(&self.lock()?.db, "desktop_preferences", WindowState::parse)
    }

    pub fn save_window_state(&self, state: WindowState) -> Result<()> {
        shellfox_core::validation::ensure(state.valid(), "Invalid desktop window bounds.")?;
        let value = serde_json::to_string(&state).map_err(storage_error)?;
        self.write_preference(
            "INSERT INTO desktop_preferences VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            value,
        )?;
        Ok(())
    }
    /// Device preferences carry no metadata revision.
    fn write_preference(&self, sql: &str, value: String) -> Result<usize> {
        let mut inner = self.lock()?;
        let crate::Inner { db, journal, .. } = &mut *inner;
        let tx = crate::journal::Write::begin(db, journal)?;
        let changed = tx.execute(sql, (KEY, value)).map_err(storage_error)?;
        tx.commit()?;
        Ok(changed)
    }

    /// Converts Electron's bounds once. Only a private snapshot is opened.
    /// Import is independent of settings revision and never replaces Rust bounds.
    pub fn import_electron_window_state(&self, snapshot: &Path) -> Result<bool> {
        let legacy = Connection::open_with_flags(
            snapshot,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(storage_error)?;
        legacy
            .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
            .map_err(storage_error)?;
        let version: i64 = legacy
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(storage_error)?;
        let table: Option<String> = legacy
            .query_row(
                "SELECT type FROM pragma_table_list WHERE schema='main' AND name='preferences'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if !(1..=5).contains(&version) || table.as_deref() != Some("table") {
            return Ok(false);
        }
        let Some(state) = read(&legacy, "preferences", WindowState::parse_electron)? else {
            return Ok(false);
        };
        let value = serde_json::to_string(&state).map_err(storage_error)?;
        let inserted = self.write_preference(
            "INSERT OR IGNORE INTO desktop_preferences VALUES (?1, ?2)",
            value,
        )?;
        Ok(inserted == 1)
    }
}

fn read(
    db: &Connection,
    table: &str,
    parse: fn(&[u8]) -> Option<WindowState>,
) -> Result<Option<WindowState>> {
    // Internal schema names only. Both tables use the same key.
    let sql = format!(
        "SELECT CAST(value AS BLOB) FROM {table} WHERE key=?1 AND length(CAST(value AS BLOB))<4096"
    );
    let bytes: Option<Vec<u8>> = db
        .query_row(&sql, [KEY], |row| row.get(0))
        .optional()
        .map_err(storage_error)?;
    Ok(bytes.and_then(|bytes| parse(&bytes)))
}

#[cfg(test)]
#[path = "window_state_tests.rs"]
mod tests;
