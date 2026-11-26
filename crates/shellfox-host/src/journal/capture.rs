//! Records the rows a write transaction changes on the memory copy, and
//! replays them on the disk file.
use crate::storage::storage_error;
use rusqlite::{types::Value, Connection};
use shellfox_core::Result;

const CAPTURE: &str = "temp.shellfox_journal";

const INSERT: i64 = 0;
const UPDATE: i64 = 1;
const DELETE: i64 = 2;

/// One recorded row change. Inserts carry the rowid and every column; updates
/// the old rowid, the new rowid and every column; deletes the old rowid. The
/// memory copy was restored from the disk file, so rowids match on both, and
/// the disk replays the same rows in the same order under the same foreign
/// keys. A row a cascade already removed on disk is simply not found again.
pub(super) struct Change {
    table: usize,
    kind: i64,
    values: Vec<Value>,
}
struct Table {
    name: String,
    columns: Vec<String>,
}
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The store's tables, read once at open: the schema never changes after migration.
pub(super) struct Schema(Vec<Table>);
impl Schema {
    pub fn read(db: &Connection) -> Result<Self> {
        let mut tables = Vec::new();
        let mut list = db
            .prepare("SELECT name, wr FROM pragma_table_list WHERE schema='main' AND type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(storage_error)?;
        let rows = list
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (name, without_rowid) = row.map_err(storage_error)?;
            if without_rowid != 0 {
                return Err(storage_error("journal needs rowid tables"));
            }
            let mut info = db
                .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
                .map_err(storage_error)?;
            let columns = info
                .query_map([&name], |row| row.get::<_, String>(0))
                .map_err(storage_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage_error)?;
            tables.push(Table { name, columns });
        }
        Ok(Self(tables))
    }
    /// Creates the capture table and the triggers that fill it, on the memory copy only.
    pub fn install(&self, db: &Connection) -> Result<()> {
        let width = self
            .0
            .iter()
            .map(|t| t.columns.len() + 2)
            .max()
            .unwrap_or(1);
        let slots: Vec<_> = (0..width).map(|i| format!("v{i}")).collect();
        let mut sql = format!(
            "CREATE TEMP TABLE shellfox_journal (tbl INTEGER NOT NULL, kind INTEGER NOT NULL, {});",
            slots.join(", ")
        );
        for (index, table) in self.0.iter().enumerate() {
            let name = quote(&table.name);
            let new: Vec<_> = table
                .columns
                .iter()
                .map(|c| format!("NEW.{}", quote(c)))
                .collect();
            let values = |prefix: &[&str], rest: &[String]| {
                let all: Vec<String> = prefix
                    .iter()
                    .map(|v| v.to_string())
                    .chain(rest.iter().cloned())
                    .collect();
                (slots[..all.len()].join(", "), all.join(", "))
            };
            let (insert_slots, insert_values) = values(&["NEW.rowid"], &new);
            let (update_slots, update_values) = values(&["OLD.rowid", "NEW.rowid"], &new);
            let (delete_slots, delete_values) = values(&["OLD.rowid"], &[]);
            sql.push_str(&format!(
                "CREATE TEMP TRIGGER shellfox_journal_{index}_insert AFTER INSERT ON main.{name} BEGIN
                    INSERT INTO {CAPTURE}(tbl, kind, {insert_slots}) VALUES({index}, {INSERT}, {insert_values}); END;
                CREATE TEMP TRIGGER shellfox_journal_{index}_update AFTER UPDATE ON main.{name} BEGIN
                    INSERT INTO {CAPTURE}(tbl, kind, {update_slots}) VALUES({index}, {UPDATE}, {update_values}); END;
                CREATE TEMP TRIGGER shellfox_journal_{index}_delete AFTER DELETE ON main.{name} BEGIN
                    INSERT INTO {CAPTURE}(tbl, kind, {delete_slots}) VALUES({index}, {DELETE}, {delete_values}); END;"
            ));
        }
        db.execute_batch(&sql).map_err(storage_error)
    }
    /// The changes recorded in the open transaction, in order; clears them.
    pub fn take(&self, tx: &Connection) -> Result<Vec<Change>> {
        let mut changes = Vec::new();
        {
            let mut stmt = tx
                .prepare_cached(&format!("SELECT * FROM {CAPTURE} ORDER BY rowid"))
                .map_err(storage_error)?;
            let width = stmt.column_count();
            let mut rows = stmt.query([]).map_err(storage_error)?;
            while let Some(row) = rows.next().map_err(storage_error)? {
                let table: usize = row.get::<_, i64>(0).map_err(storage_error)? as usize;
                let kind: i64 = row.get(1).map_err(storage_error)?;
                let columns = self
                    .0
                    .get(table)
                    .ok_or_else(|| storage_error("table"))?
                    .columns
                    .len();
                let count = match kind {
                    INSERT => columns + 1,
                    UPDATE => columns + 2,
                    _ => 1,
                };
                let values = (2..(2 + count).min(width))
                    .map(|i| row.get::<_, Value>(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(storage_error)?;
                changes.push(Change {
                    table,
                    kind,
                    values,
                });
            }
        }
        if !changes.is_empty() {
            tx.execute(&format!("DELETE FROM {CAPTURE}"), [])
                .map_err(storage_error)?;
        }
        Ok(changes)
    }
    /// Replays changes on the disk file, in one transaction.
    pub fn apply(&self, disk: &Connection, batches: &[Vec<Change>]) -> rusqlite::Result<()> {
        let tx = disk.unchecked_transaction()?;
        for change in batches.iter().flatten() {
            let table = &self.0[change.table];
            let name = quote(&table.name);
            let columns: Vec<_> = table.columns.iter().map(|c| quote(c)).collect();
            let params = rusqlite::params_from_iter(change.values.iter());
            match change.kind {
                INSERT => {
                    let marks = vec!["?"; columns.len() + 1].join(", ");
                    tx.prepare_cached(&format!(
                        "INSERT OR REPLACE INTO {name}(rowid, {}) VALUES({marks})",
                        columns.join(", ")
                    ))?
                    .execute(params)?;
                }
                UPDATE => {
                    let set: Vec<_> = columns
                        .iter()
                        .enumerate()
                        .map(|(i, c)| format!("{c}=?{}", i + 3))
                        .collect();
                    let updated = tx
                        .prepare_cached(&format!(
                            "UPDATE {name} SET rowid=?2, {} WHERE rowid=?1",
                            set.join(", ")
                        ))?
                        .execute(params)?;
                    if updated == 0 {
                        return Err(rusqlite::Error::QueryReturnedNoRows);
                    }
                }
                _ => {
                    tx.prepare_cached(&format!("DELETE FROM {name} WHERE rowid=?1"))?
                        .execute(params)?;
                }
            }
        }
        tx.commit()
    }
}
