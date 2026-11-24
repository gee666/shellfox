//! Owner-only debounce/commit helpers. No terminal delivery or second Host lives here.
use super::{AcceptedInputContext, ActivityUtcAnchor, InputActivity, LiveActivityUpdate};
use crate::activity::{SessionActivity, SessionAging, LIFECYCLE_INTERVAL_MS};
use shellfox_core::{SessionId, Snapshot, TerminalIdentity};
use shellfox_host::{CapturedSettlement, Host};
use shellfox_runtime::Identity;
use std::collections::{HashMap, VecDeque};
#[cfg(test)]
mod tests;

const MAX_PENDING_SESSIONS: usize = 10_000;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityPersistenceFailure {
    Bounds,
    InvalidClock,
    Storage,
}
/// Deliberately NOT a Result: metadata failure cannot become a failed PTY write.
#[derive(Default)]
pub struct AcceptedInputMetadata {
    pub resumed: bool,
    pub snapshot: Option<Snapshot>,
    pub metadata_error: Option<ActivityPersistenceFailure>,
}
pub enum ActivityFlush {
    Idle,
    Committed {
        session_id: SessionId,
        snapshot: Snapshot,
    },
    Deferred {
        session_id: SessionId,
        error: ActivityPersistenceFailure,
    },
}
pub struct ActivityPersistence {
    utc: ActivityUtcAnchor,
    pending: HashMap<SessionId, SessionActivity>,
    cycle: VecDeque<SessionId>,
    next_flush: u64,
}
impl ActivityPersistence {
    pub fn new(utc: ActivityUtcAnchor) -> Self {
        Self {
            utc,
            pending: HashMap::new(),
            cycle: VecDeque::new(),
            next_flush: utc.logical_ms.saturating_add(LIFECYCLE_INTERVAL_MS),
        }
    }
    /// The earliest logical time [`Self::flush_next`] has work. A started
    /// cycle commits one session per owner pass, so it is due at once.
    pub fn next_flush_ms(&self) -> Option<u64> {
        if !self.cycle.is_empty() {
            Some(0)
        } else if self.pending.is_empty() {
            None
        } else {
            Some(self.next_flush)
        }
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    /// Touches not yet written to the session's `updatedAt`.
    pub fn pending(&self, session: SessionId) -> Option<&SessionActivity> {
        self.pending.get(&session)
    }
    /// A stored UTC timestamp on the owner's logical clock.
    pub fn logical_ms(&self, at: &str) -> Option<u64> {
        self.utc
            .from_unix_ms(shellfox_host::activity_unix_ms(at).ok()?)
    }
    pub fn touch(
        &mut self,
        session: SessionId,
        at_ms: u64,
    ) -> Result<(), ActivityPersistenceFailure> {
        if !self.pending.contains_key(&session) && self.pending.len() >= MAX_PENDING_SESSIONS {
            return Err(ActivityPersistenceFailure::Bounds);
        }
        self.pending.entry(session).or_default().touch(at_ms);
        Ok(())
    }
    /// All live data, including idle/echo. Meaningful-output time is busy telemetry.
    pub fn live_updates(
        &mut self,
        updates: &[LiveActivityUpdate],
    ) -> Result<(), ActivityPersistenceFailure> {
        for update in updates {
            if let Some(at) = update.output_at_ms {
                self.touch(update.identity.session_id, at)?;
            }
        }
        Ok(())
    }
    pub fn forget_session(&mut self, session: SessionId) {
        self.pending.remove(&session);
        self.cycle.retain(|queued| *queued != session);
    }
    pub fn accepted_receipt(
        &mut self,
        host: &Host,
        accepted: AcceptedInputContext,
        at_ms: u64,
    ) -> AcceptedInputMetadata {
        if accepted.receipt.accepted_bytes == 0 {
            return AcceptedInputMetadata::default();
        }
        self.accepted_input(
            host,
            accepted.identity,
            accepted.activity,
            accepted.admission.captured_settlement,
            at_ms,
        )
    }
    /// Called AFTER successful, meaningful user delivery. Touch first, then let the
    /// Host atomically compare the captured marker/current generation. Caller always
    /// preserves its successful write response, even if metadata_error is returned.
    pub fn accepted_input(
        &mut self,
        host: &Host,
        identity: Identity,
        input: InputActivity,
        captured: CapturedSettlement,
        at_ms: u64,
    ) -> AcceptedInputMetadata {
        if !input.touch_session {
            return AcceptedInputMetadata::default();
        }
        if let Err(error) = self.touch(identity.session_id, at_ms) {
            return AcceptedInputMetadata {
                metadata_error: Some(error),
                ..AcceptedInputMetadata::default()
            };
        }
        let at = match self.timestamp(at_ms) {
            Ok(at) => at,
            Err(error) => {
                return AcceptedInputMetadata {
                    metadata_error: Some(error),
                    ..AcceptedInputMetadata::default()
                }
            }
        };
        match host.resume_session_from_input(
            TerminalIdentity {
                tab_id: identity.tab_id,
                generation: identity.generation,
            },
            captured,
            &at,
        ) {
            Ok(result) if result.resumed => {
                self.committed(identity.session_id, at_ms);
                AcceptedInputMetadata {
                    resumed: true,
                    snapshot: Some(result.snapshot),
                    metadata_error: None,
                }
            }
            Ok(_) => AcceptedInputMetadata::default(),
            Err(_) => AcceptedInputMetadata {
                metadata_error: Some(ActivityPersistenceFailure::Storage),
                ..AcceptedInputMetadata::default()
            },
        }
    }
    /// One metadata transaction per owner tick. Minute cycles cover pending IDs
    /// once each; a failed session cannot starve other sessions. Shutdown passes
    /// force=true and retries deferred touches while retaining the same owner.
    pub fn flush_next(&mut self, host: &Host, now_ms: u64, force: bool) -> ActivityFlush {
        if self.cycle.is_empty() {
            if !force && now_ms < self.next_flush {
                return ActivityFlush::Idle;
            }
            self.next_flush = now_ms.saturating_add(LIFECYCLE_INTERVAL_MS);
            let mut ids: Vec<_> = self.pending.keys().copied().collect();
            ids.sort();
            self.cycle.extend(ids);
        }
        let Some(session_id) = self.cycle.pop_front() else {
            return ActivityFlush::Idle;
        };
        let Some(activity) = self.pending.get(&session_id) else {
            return ActivityFlush::Idle;
        };
        let through = activity.last_action_ms(SessionAging {
            updated_ms: 0,
            settled_ms: None,
            pinned: false,
        });
        let at = match self.timestamp(through) {
            Ok(at) => at,
            Err(error) => return ActivityFlush::Deferred { session_id, error },
        };
        match host.record_session_activity(session_id, &at) {
            Ok(snapshot) => {
                self.committed(session_id, through);
                ActivityFlush::Committed {
                    session_id,
                    snapshot,
                }
            }
            Err(_) => ActivityFlush::Deferred {
                session_id,
                error: ActivityPersistenceFailure::Storage,
            },
        }
    }
    fn timestamp(&self, logical_ms: u64) -> Result<String, ActivityPersistenceFailure> {
        let unix_ms = self
            .utc
            .to_unix_ms(logical_ms)
            .ok_or(ActivityPersistenceFailure::InvalidClock)?;
        shellfox_host::activity_utc_timestamp(unix_ms)
            .map_err(|_| ActivityPersistenceFailure::InvalidClock)
    }
    fn committed(&mut self, session: SessionId, through: u64) {
        if let Some(activity) = self.pending.get_mut(&session) {
            activity.persisted(through);
            if activity.pending_update(through).is_none() {
                self.pending.remove(&session);
            }
        }
    }
}
