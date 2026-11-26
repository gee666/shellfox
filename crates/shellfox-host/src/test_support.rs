//! Tests change the working copy here, the way a Host write would, instead of
//! editing the disk file behind the Host's back: the Host never reads it again.
use crate::{journal::Write, storage::storage_error, Host, Inner, Result};

impl Host {
    /// Runs one statement on the working copy as a journaled write.
    pub fn test_write(&self, sql: &str, params: impl rusqlite::Params) -> Result<usize> {
        let mut inner = self.lock()?;
        let Inner { db, journal, .. } = &mut *inner;
        let tx = Write::begin(db, journal)?;
        let changed = tx.execute(sql, params).map_err(storage_error)?;
        tx.commit()?;
        Ok(changed)
    }
    /// Runs SQL on the working copy only, such as a trigger that injects a fault.
    pub fn test_batch(&self, sql: &str) -> Result<()> {
        self.lock()?.db.execute_batch(sql).map_err(storage_error)
    }
}
