//! Trusted owner activity persistence. Input delivery is a separate operation:
//! metadata failure must never cause an already accepted PTY write to be replayed.
use crate::{sessions::ensure_not_prepared, storage, *};
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};

/// Host-only input provenance, not persisted metadata or a wire capability.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CapturedSettlement {
    /// An explicitly supported legacy client omitted its marker.
    Legacy,
    /// Admission could not capture the marker. Never infer one at completion.
    Unavailable,
    /// Known admission-time state, including a known null marker.
    Captured(Option<String>),
}
impl From<Option<Option<String>>> for CapturedSettlement {
    fn from(marker: Option<Option<String>>) -> Self {
        match marker {
            None => Self::Legacy,
            Some(marker) => Self::Captured(marker),
        }
    }
}
#[derive(Clone, Debug)]
pub struct ResumeSessionResult {
    pub snapshot: Snapshot,
    pub resumed: bool,
}
impl Host {
    /// Explicit user metadata, including for live owned tabs. This flag is not
    /// idle/process-exit proof and never stops, unregisters or cleans up a runtime.
    pub fn set_session_settled(&self, id: SessionId, settled: bool) -> Result<Snapshot> {
        self.activity_transaction(|data, registry| {
            let session = data.session(id)?;
            check_prepared(registry, session)?;
            let already_settled = session.settled_at.is_some();
            let marker = if settled && !already_settled {
                let prior = registry.last_settlement_ms;
                let next = activity_unix_ms(&now())?.max(
                    prior
                        .checked_add(1)
                        .ok_or_else(|| storage::storage_error("settlement clock overflow"))?,
                );
                let marker = activity_utc_timestamp(next)?;
                registry.last_settlement_ms = next;
                Some(marker)
            } else {
                None
            };
            let session = data.session_mut(id)?;
            if !settled {
                session.settled_at = None;
            } else if !already_settled {
                session.settled_at = marker;
            }
            // Electron's explicit mutate always touches activity and publishes a
            // revision. Repeated settle preserves the existing captured marker.
            session.updated_at = now();
            Ok(true)
        })
        .map(|(snapshot, _)| snapshot)
    }
    /// Flush an owner-debounced touch, normally once per minute or at shutdown.
    /// Only acknowledge the captured pending touch AFTER this returns Ok. A newer
    /// pending touch must survive. Older/equal instants perform no metadata write.
    pub fn record_session_activity(&self, id: SessionId, at: &str) -> Result<Snapshot> {
        let parsed = activity_timestamp(at)?;
        self.activity_transaction(|data, registry| {
            let session = data.session_mut(id)?;
            check_prepared(registry, session)?;
            if advances(&session.updated_at, parsed)? {
                session.updated_at = at.to_owned();
                Ok(true)
            } else {
                Ok(false)
            }
        })
        .map(|(snapshot, _)| snapshot)
    }
    /// Call only AFTER meaningful user input was accepted by this owned generation.
    /// Always touch owner-pending activity first, even for unavailable, stale or
    /// captured-null input. Unavailable never authorizes a settlement mutation.
    /// Only an actual settlement transition writes immediately. Otherwise the owner
    /// keeps the touch in memory for record_session_activity, not one write per key.
    /// An Err here is metadata failure, NEVER a failed/retryable PTY delivery.
    pub fn resume_session_from_input(
        &self,
        identity: TerminalIdentity,
        captured: CapturedSettlement,
        at: &str,
    ) -> Result<ResumeSessionResult> {
        let parsed = activity_timestamp(at)?;
        if let CapturedSettlement::Captured(Some(marker)) = &captured {
            validation::timestamp(marker)?;
        }
        let (snapshot, resumed) = self.activity_transaction(|data, registry| {
            if registry.live.get(&identity.tab_id) != Some(&identity.generation) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Terminal generation is no longer current.",
                ));
            }
            let session_id = data.tab(identity.tab_id)?.session_id;
            let session = data.session_mut(session_id)?;
            check_prepared(registry, session)?;
            let matches = session.settled_at.is_some()
                && match &captured {
                    CapturedSettlement::Legacy => true,
                    CapturedSettlement::Unavailable => false,
                    CapturedSettlement::Captured(marker) => marker == &session.settled_at,
                };
            if !matches {
                return Ok(false);
            }
            session.settled_at = None;
            if advances(&session.updated_at, parsed)? {
                session.updated_at = at.to_owned();
            }
            Ok(true)
        })?;
        Ok(ResumeSessionResult { snapshot, resumed })
    }
    fn activity_transaction(
        &self,
        action: impl FnOnce(&mut storage::Data, &mut Registry) -> Result<bool>,
    ) -> Result<(Snapshot, bool)> {
        let mut inner = self.lock()?;
        let runtime = inner.settings_runtime.clone();
        let Inner { db, registry, .. } = &mut *inner;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage::storage_error)?;
        let mut data = storage::load(&tx)?;
        registry.last_settlement_ms = registry
            .last_settlement_ms
            .max(latest_settlement_ms(&data.sessions)?);
        let changed = action(&mut data, registry)?;
        if changed {
            data.state.revision = data
                .state
                .revision
                .checked_add(1)
                .ok_or_else(|| storage::storage_error("revision overflow"))?;
            data.state.last_change = Some(ChangedEvent {
                revision: data.state.revision,
                reason: ChangeReason::Sessions,
                select_session_id: data.state.selection.session_id,
            });
            storage::save(&tx, &data)?;
        }
        let snapshot = data.snapshot();
        if let Some(runtime) = runtime {
            runtime.check_commit()?;
        }
        self.commit(tx, snapshot.revision)?;
        Ok((snapshot, changed))
    }
}
/// Parse validated UTC activity time into unsigned Unix milliseconds.
/// Sub-millisecond precision is truncated, matching JavaScript Date precision.
pub fn activity_unix_ms(at: &str) -> Result<u64> {
    u64::try_from(activity_timestamp(at)?.timestamp_millis()).map_err(|_| {
        Error::new(
            ErrorCode::Validation,
            "Activity time precedes the Unix epoch.",
        )
    })
}
/// Format checked Unix milliseconds as canonical UTC with exactly three decimals.
pub fn activity_utc_timestamp(at: u64) -> Result<String> {
    let millis = i64::try_from(at).map_err(|_| activity_range_error())?;
    let timestamp = DateTime::<Utc>::from_timestamp_millis(millis)
        .ok_or_else(activity_range_error)?
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    validation::timestamp(&timestamp)?;
    Ok(timestamp)
}
pub(super) fn latest_settlement_ms(sessions: &[Session]) -> Result<u64> {
    sessions
        .iter()
        .filter_map(|session| session.settled_at.as_deref())
        .try_fold(0, |latest, marker| {
            let millis = DateTime::parse_from_rfc3339(marker)
                .map_err(storage::storage_error)?
                .timestamp_millis();
            // Valid pre-epoch historical markers cannot advance the unsigned clock.
            Ok(latest.max(u64::try_from(millis).unwrap_or_default()))
        })
}
fn activity_range_error() -> Error {
    Error::new(
        ErrorCode::Validation,
        "Activity time is outside the supported UTC timestamp range.",
    )
}
fn activity_timestamp(at: &str) -> Result<DateTime<FixedOffset>> {
    validation::timestamp(at)?;
    DateTime::parse_from_rfc3339(at)
        .map_err(|_| Error::new(ErrorCode::Validation, "Expected a UTC activity timestamp."))
}
fn advances(current: &str, at: DateTime<FixedOffset>) -> Result<bool> {
    Ok(at > DateTime::parse_from_rfc3339(current).map_err(storage::storage_error)?)
}
fn check_prepared(registry: &Registry, session: &Session) -> Result<()> {
    ensure_not_prepared(
        registry,
        &session.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
        Some(session.id),
    )
}
