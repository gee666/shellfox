//! Trusted publication of external automation intent. Never Runtime ownership proof.
use crate::{now, storage, *};
use rusqlite::Connection;

/// Service supplies its immutable reserved IDs only at Show, after checking its
/// already-owned Runtime's exact session, tab and generation. Not a wire DTO.
#[derive(Clone, Debug)]
pub struct ReservedSessionPublication {
    pub request_id: SessionId,
    pub session_id: SessionId,
    pub tab_id: TabId,
    pub title: Option<String>,
    pub cwd: String,
    pub profile_id: ProfileId,
}

/// Historical metadata intent only. This proves no process, generation, launch
/// success, current selection or permission to adopt a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservedPublicationReceipt {
    Published {
        session_id: SessionId,
        tab_id: TabId,
    },
    TargetDeleted {
        session_id: SessionId,
        tab_id: TabId,
    },
    ReceiptUnavailable,
}

impl Host {
    pub fn reserved_publication_receipt(
        &self,
        request_id: SessionId,
    ) -> Result<ReservedPublicationReceipt> {
        let inner = self.lock()?;
        let tx = inner
            .db
            .unchecked_transaction()
            .map_err(storage::storage_error)?;
        let data = storage::load(&tx)?;
        let receipt = receipt(&tx, &data, request_id)?;
        tx.commit().map_err(storage::storage_error)?;
        Ok(receipt)
    }

    /// Publish exact reserved IDs, without selecting or registering a generation.
    /// Some facts require a verified catalog profile captured for the original
    /// run. Raw executables use None, never facts inferred from current settings.
    /// Retries preserve original attributes and fact provenance, including None.
    pub fn publish_reserved_session(
        &self,
        input: ReservedSessionPublication,
        facts: Option<SessionCreationFacts>,
    ) -> Result<Snapshot> {
        validation::cwd(&input.cwd)?;
        if let Some(title) = &input.title {
            validation::title(title)?;
        }
        if let Some(facts) = &facts {
            facts.validate()?;
        }
        let mut inner = self.lock()?;
        let runtime = inner.settings_runtime.clone();
        let tx = inner
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage::storage_error)?;
        let mut data = storage::load(&tx)?;
        match receipt(&tx, &data, input.request_id)? {
            ReservedPublicationReceipt::Published { session_id, tab_id }
            | ReservedPublicationReceipt::TargetDeleted { session_id, tab_id }
                if session_id != input.session_id || tab_id != input.tab_id =>
            {
                return Err(collision());
            }
            ReservedPublicationReceipt::TargetDeleted { .. } => {
                return Err(storage::not_found());
            }
            ReservedPublicationReceipt::Published { .. } => {}
            ReservedPublicationReceipt::ReceiptUnavailable => {
                if data.requests.iter().any(|(id, _)| *id == input.request_id)
                    || data
                        .sessions
                        .iter()
                        .any(|session| session.id == input.session_id)
                    || data
                        .sessions
                        .iter()
                        .flat_map(|session| &session.tabs)
                        .any(|tab| tab.id == input.tab_id)
                    || reserved_ids(&tx)?
                        .iter()
                        .any(|(_, sid, tid)| *sid == input.session_id || *tid == input.tab_id)
                {
                    return Err(collision());
                }
                let time = now();
                let tab = Tab {
                    id: input.tab_id,
                    session_id: input.session_id,
                    title: input.profile_id.to_string(),
                    cwd: input.cwd.clone(),
                    ordinal: 0,
                    created_at: Some(time.clone()),
                    profile_id: input.profile_id,
                    state: TabState::Pending,
                };
                data.sessions.push(Session {
                    id: input.session_id,
                    title: input
                        .title
                        .unwrap_or_else(|| crate::sessions::directory_title(&input.cwd)),
                    cwd: input.cwd,
                    created_at: time.clone(),
                    updated_at: time,
                    settled_at: None,
                    pinned_at: None,
                    env: vec![],
                    tabs: vec![tab],
                });
                if let Some(facts) = facts {
                    data.creation_facts.insert(input.session_id, facts);
                }
                data.state.revision = data
                    .state
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| storage::storage_error("revision overflow"))?;
                data.state.last_change = Some(ChangedEvent {
                    revision: data.state.revision,
                    reason: ChangeReason::Sessions,
                    select_session_id: None,
                });
                storage::save(&tx, &data)?;
                tx.execute(
                    "INSERT INTO reserved_publications VALUES(1,?,?,?)",
                    rusqlite::params![
                        input.request_id.to_string(),
                        input.session_id.to_string(),
                        input.tab_id.to_string()
                    ],
                )
                .map_err(storage::storage_error)?;
            }
        }
        let snapshot = data.snapshot();
        if let Some(runtime) = runtime {
            runtime.check_commit()?;
        }
        tx.commit().map_err(storage::storage_error)?;
        Ok(snapshot)
    }
}
fn collision() -> Error {
    Error::new(
        ErrorCode::Conflict,
        "Reserved publication identity conflicts with recorded intent.",
    )
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    // No FK: save rewrites sessions/tabs, and actual deletion must retain a
    // tombstone. The receipt must never disappear through a cascade.
    db.execute_batch(
        "CREATE TABLE reserved_publications (
        version INTEGER NOT NULL CHECK(version=1),
        request_id TEXT PRIMARY KEY NOT NULL,
        session_id TEXT NOT NULL UNIQUE,
        tab_id TEXT NOT NULL UNIQUE
    );",
    )
    .map_err(storage::storage_error)
}
fn reserved_ids(db: &Connection) -> Result<Vec<(SessionId, SessionId, TabId)>> {
    let mut statement = db
        .prepare("SELECT version,request_id,session_id,tab_id FROM reserved_publications")
        .map_err(storage::storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(storage::storage_error)?;
    let mut result = Vec::new();
    for row in rows {
        let (version, request, session, tab) = row.map_err(storage::storage_error)?;
        let ids: (SessionId, SessionId, TabId) = (
            request.parse().map_err(storage::storage_error)?,
            session.parse().map_err(storage::storage_error)?,
            tab.parse().map_err(storage::storage_error)?,
        );
        if version != 1
            || ids.0.to_string() != request
            || ids.1.to_string() != session
            || ids.2.to_string() != tab
        {
            return Err(storage::storage_error("invalid reserved publication"));
        }
        result.push(ids);
    }
    Ok(result)
}
fn receipt(
    db: &Connection,
    data: &storage::Data,
    request_id: SessionId,
) -> Result<ReservedPublicationReceipt> {
    Ok(
        match reserved_ids(db)?
            .into_iter()
            .find(|(id, _, _)| *id == request_id)
        {
            Some((_, session_id, tab_id)) => {
                if data.sessions.iter().any(|session| {
                    session.id == session_id && session.tabs.iter().any(|tab| tab.id == tab_id)
                }) {
                    ReservedPublicationReceipt::Published { session_id, tab_id }
                } else {
                    ReservedPublicationReceipt::TargetDeleted { session_id, tab_id }
                }
            }
            None => ReservedPublicationReceipt::ReceiptUnavailable,
        },
    )
}

pub(crate) fn ensure_request_available(db: &Connection, request_id: SessionId) -> Result<()> {
    if reserved_ids(db)?.iter().any(|(id, _, _)| *id == request_id) {
        return Err(collision());
    }
    Ok(())
}

pub(crate) fn validate_store(db: &Connection, data: &storage::Data) -> Result<()> {
    for (request, session, tab) in reserved_ids(db)? {
        if data.requests.iter().any(|(id, _)| *id == request)
            || data
                .sessions
                .iter()
                .flat_map(|session| &session.tabs)
                .any(|entry| entry.id == tab && entry.session_id != session)
        {
            return Err(storage::storage_error("conflicting reserved publication"));
        }
    }
    Ok(())
}
