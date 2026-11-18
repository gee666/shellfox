//! Approval transactions on the Host's existing connection and mutex.
use crate::*;
use rusqlite::{params, Connection, TransactionBehavior};
const POLICY: &str = "control-full-account-v1";
fn storage_error(error: impl std::fmt::Display) -> Error {
    let mut error = crate::storage::storage_error(error);
    error.retryable = false;
    error
}

pub(crate) fn configure(db: &Connection) -> Result<()> {
    db.pragma_update(None, "synchronous", "FULL")
        .map_err(storage_error)?;
    full(db)
}
fn full(db: &Connection) -> Result<()> {
    let value: i64 = db
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(storage_error)?;
    validation::ensure(
        value == 2,
        "Approval commits require SQLite FULL synchronization.",
    )
}
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let existing: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name GLOB 'native_approval*'",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    validation::ensure(
        existing == 0,
        "Unexpected approval schema in a predecessor store.",
    )?;
    db.execute_batch("CREATE TABLE native_approval_state (
        id INTEGER PRIMARY KEY CHECK(id=1), feature_version INTEGER NOT NULL CHECK(feature_version=1),
        policy TEXT NOT NULL CHECK(policy='control-full-account-v1'),
        revision INTEGER NOT NULL CHECK(revision>=0),
        phase TEXT NOT NULL CHECK(phase IN ('committed','pending')));
        CREATE TABLE native_approvals (
        peer_id TEXT PRIMARY KEY NOT NULL, hostname TEXT NOT NULL,
        approved_at_millis INTEGER NOT NULL CHECK(approved_at_millis>=0),
        policy TEXT NOT NULL CHECK(policy='control-full-account-v1'));
        INSERT INTO native_approval_state VALUES(1,1,'control-full-account-v1',0,'committed');")
        .map_err(storage_error)?;
    read(db)?;
    Ok(())
}
fn schema(db: &Connection) -> Result<()> {
    for (table, expected) in [
        (
            "native_approval_state",
            &[
                ("id", "INTEGER", 0, 1),
                ("feature_version", "INTEGER", 1, 0),
                ("policy", "TEXT", 1, 0),
                ("revision", "INTEGER", 1, 0),
                ("phase", "TEXT", 1, 0),
            ][..],
        ),
        (
            "native_approvals",
            &[
                ("peer_id", "TEXT", 1, 1),
                ("hostname", "TEXT", 1, 0),
                ("approved_at_millis", "INTEGER", 1, 0),
                ("policy", "TEXT", 1, 0),
            ][..],
        ),
    ] {
        let mut stmt = db
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(storage_error)?;
        let mut rows = stmt.query([]).map_err(storage_error)?;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(storage_error)? {
            let name: String = row.get(1).map_err(storage_error)?;
            let kind: String = row.get(2).map_err(storage_error)?;
            let not_null: i64 = row.get(3).map_err(storage_error)?;
            let default: Option<String> = row.get(4).map_err(storage_error)?;
            let key: i64 = row.get(5).map_err(storage_error)?;
            validation::ensure(
                expected.get(count).copied() == Some((name.as_str(), kind.as_str(), not_null, key))
                    && default.is_none(),
                "Invalid approval schema.",
            )?;
            count += 1;
        }
        validation::ensure(
            count == expected.len(),
            "Required approval schema is missing.",
        )?;
    }
    Ok(())
}
fn meta(db: &Connection) -> Result<(u64, String)> {
    schema(db)?;
    let count: i64 = db
        .query_row("SELECT count(*) FROM native_approval_state", [], |r| {
            r.get(0)
        })
        .map_err(storage_error)?;
    validation::ensure(
        count == 1,
        "Required approval state is missing or duplicated.",
    )?;
    let valid: i64 = db.query_row("SELECT count(*) FROM native_approval_state WHERE id=1 AND feature_version=1
        AND typeof(feature_version)='integer' AND policy COLLATE BINARY='control-full-account-v1' AND typeof(policy)='text'
        AND typeof(revision)='integer' AND revision>=0 AND typeof(phase)='text' AND phase IN ('committed','pending')", [], |r| r.get(0)).map_err(storage_error)?;
    validation::ensure(valid == 1, "Approval feature or policy is invalid.")?;
    let (revision, phase): (i64, String) = db
        .query_row(
            "SELECT revision,phase FROM native_approval_state WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage_error)?;
    Ok((u64::try_from(revision).map_err(storage_error)?, phase))
}
fn read(db: &Connection) -> Result<ApprovalStoreSnapshot> {
    full(db)?;
    let (revision, phase) = meta(db)?;
    validation::ensure(
        phase == "committed",
        "Approval intent is pending; authority is unavailable.",
    )?;
    let count: i64 = db
        .query_row("SELECT count(*) FROM native_approvals", [], |r| r.get(0))
        .map_err(storage_error)?;
    let count = usize::try_from(count).map_err(storage_error)?;
    validation::ensure(count <= APPROVAL_RECORD_LIMIT, "Too many saved approvals.")?;
    let invalid: i64 = db.query_row("SELECT count(*) FROM native_approvals WHERE typeof(peer_id)!='text'
        OR length(CAST(peer_id AS BLOB)) NOT BETWEEN 1 AND 200 OR typeof(hostname)!='text'
        OR length(CAST(hostname AS BLOB)) NOT BETWEEN 1 AND 253 OR typeof(approved_at_millis)!='integer'
        OR approved_at_millis<0 OR approved_at_millis>9007199254740991
        OR typeof(policy)!='text' OR policy COLLATE BINARY!='control-full-account-v1'", [], |r| r.get(0)).map_err(storage_error)?;
    validation::ensure(invalid == 0, "Saved approval data is invalid.")?;
    let mut stmt = db
        .prepare(
            "SELECT peer_id,hostname,approved_at_millis FROM native_approvals ORDER BY peer_id COLLATE BINARY",
        )
        .map_err(storage_error)?;
    let mut rows = stmt.query([]).map_err(storage_error)?;
    let mut approvals = Vec::with_capacity(count);
    while let Some(row) = rows.next().map_err(storage_error)? {
        let timestamp: i64 = row.get(2).map_err(storage_error)?;
        approvals.push(ApprovalStoreRecord {
            peer_id: row.get(0).map_err(storage_error)?,
            hostname: row.get(1).map_err(storage_error)?,
            approved_at_millis: u64::try_from(timestamp).map_err(storage_error)?,
            policy: ApprovalStorePolicy::ControlFullAccountV1,
        });
    }
    let snapshot = ApprovalStoreSnapshot {
        revision,
        approvals,
    };
    snapshot.validate()?;
    Ok(snapshot)
}
impl Host {
    pub fn approval_snapshot(&self) -> Result<ApprovalStoreSnapshot> {
        let mut inner = self.lock()?;
        let tx = inner.db.transaction().map_err(storage_error)?;
        let snapshot = read(&tx)?;
        full(&tx)?;
        tx.commit().map_err(storage_error)?;
        Ok(snapshot)
    }
    /// A failed final transaction leaves the separately committed pending intent.
    /// A failed initial intent does not establish durable revocation.
    pub fn commit_approvals(
        &self,
        expected_revision: u64,
        mut approvals: Vec<ApprovalStoreRecord>,
    ) -> Result<ApprovalStoreSnapshot> {
        approvals.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
        let next = expected_revision
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(|| storage_error("approval revision exhausted"))?;
        let wanted = ApprovalStoreSnapshot {
            revision: next,
            approvals,
        };
        wanted.validate()?;
        let next_sql = i64::try_from(next).map_err(storage_error)?;
        let expected_sql = i64::try_from(expected_revision).map_err(storage_error)?;
        let mut inner = self.lock()?;
        full(&inner.db)?;
        let intent = inner
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let current = read(&intent)?;
        if current.revision != expected_revision {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Approval revision changed; no retry was made.",
            ));
        }
        let changed = intent.execute("UPDATE native_approval_state SET revision=?,phase='pending' WHERE id=1 AND revision=? AND phase='committed'",
            params![next_sql, expected_sql]).map_err(storage_error)?;
        validation::ensure(changed == 1, "Approval intent was not recorded.")?;
        full(&intent)?;
        intent.commit().map_err(storage_error)?;
        let final_tx = inner
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let (revision, phase) = meta(&final_tx)?;
        validation::ensure(
            revision == next && phase == "pending",
            "Original approval intent changed.",
        )?;
        final_tx
            .execute("DELETE FROM native_approvals", [])
            .map_err(storage_error)?;
        for record in &wanted.approvals {
            final_tx
                .execute(
                    "INSERT INTO native_approvals VALUES(?,?,?,?)",
                    params![
                        record.peer_id,
                        record.hostname,
                        i64::try_from(record.approved_at_millis).map_err(storage_error)?,
                        POLICY
                    ],
                )
                .map_err(storage_error)?;
        }
        let changed = final_tx.execute("UPDATE native_approval_state SET phase='committed' WHERE id=1 AND revision=? AND phase='pending'", [next_sql]).map_err(storage_error)?;
        validation::ensure(changed == 1, "Approval finalization was not recorded.")?;
        full(&final_tx)?;
        final_tx.commit().map_err(storage_error)?;
        let confirmed = read(&inner.db)?;
        validation::ensure(confirmed == wanted, "Approval commit read-back differs.")?;
        Ok(confirmed)
    }
}
