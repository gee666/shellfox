//! Automatic session lifecycle, as in 0.9. An unpinned session settles after
//! `sessionSettleHours` without activity; a settled one is deleted, its
//! terminals closed first, after `sessionDeleteHours` more. The owner sleeps
//! until the earliest due time computed from the stored sessions.
use crate::{
    activity::{
        integration::ActivityPersistence, ExpiryAction, SessionActivity, SessionAging,
        LIFECYCLE_INTERVAL_MS,
    },
    close::CloseTarget,
    Service,
};
use shellfox_core::{Session, SessionId, Settings, Snapshot};
use shellfox_runtime::Identity;
use std::{
    sync::mpsc::{self, TryRecvError},
    time::Instant,
};
#[cfg(test)]
mod tests;

/// How long the process refresh before an expiry may take. Without fresh
/// evidence for every live tab the sweep retries one interval later.
const REFRESH_LIMIT_MS: u64 = 10_000;
/// Recheck pace while that refresh waits for its inspections.
const REFRESH_STEP_MS: u64 = 100;

/// A process refresh in progress before an expiry.
#[derive(Clone, Copy)]
struct Refresh {
    since_ms: u64,
    next_ms: u64,
    until_ms: u64,
}
pub(crate) struct Retention {
    /// Host revision `due_ms` was computed from.
    revision: Option<u64>,
    /// The earliest settle or delete time; None when nothing can expire.
    due_ms: Option<u64>,
    /// No sweep earlier: one interval after start, as 0.9's first sweep, and
    /// one interval after a failed attempt.
    not_before_ms: u64,
    refresh: Option<Refresh>,
    deleting: Option<mpsc::Receiver<anyhow::Result<Snapshot>>>,
}
impl Retention {
    pub(crate) fn new(now_ms: u64) -> Self {
        Self {
            revision: None,
            due_ms: None,
            not_before_ms: now_ms.saturating_add(LIFECYCLE_INTERVAL_MS),
            refresh: None,
            deleting: None,
        }
    }
    fn next_ms(&self) -> Option<u64> {
        match self.refresh {
            Some(refresh) => Some(refresh.next_ms),
            None => self.due_ms.map(|due| due.max(self.not_before_ms)),
        }
    }
    fn retry_later(&mut self, now_ms: u64) {
        self.refresh = None;
        self.not_before_ms = now_ms.saturating_add(LIFECYCLE_INTERVAL_MS);
    }
}

/// A stored session on the owner's clock.
fn aging(session: &Session, persistence: &ActivityPersistence) -> Option<SessionAging> {
    Some(SessionAging {
        updated_ms: persistence.logical_ms(&session.updated_at)?,
        settled_ms: match &session.settled_at {
            Some(at) => Some(persistence.logical_ms(at)?),
            None => None,
        },
        pinned: session.pinned_at.is_some(),
    })
}
/// The earliest settle or delete time over all sessions, counting touches
/// not yet written.
pub(crate) fn next_due(
    sessions: &[Session],
    settings: &Settings,
    persistence: &ActivityPersistence,
) -> Option<u64> {
    let idle = SessionActivity::new();
    sessions
        .iter()
        .filter_map(|session| {
            let activity = persistence.pending(session.id).unwrap_or(&idle);
            activity.deadline_ms(aging(session, persistence)?, settings)
        })
        .min()
}
/// What each session due at `now_ms` gets.
pub(crate) fn expiries(
    sessions: &[Session],
    settings: &Settings,
    persistence: &ActivityPersistence,
    now_ms: u64,
) -> Vec<(SessionId, ExpiryAction)> {
    let idle = SessionActivity::new();
    sessions
        .iter()
        .filter_map(|session| {
            let activity = persistence.pending(session.id).unwrap_or(&idle);
            let action = activity.expiry(aging(session, persistence)?, settings, now_ms, true);
            (action != ExpiryAction::None).then_some((session.id, action))
        })
        .collect()
}

impl Service {
    /// Selecting a session counts as activity.
    pub(crate) fn touch_session(&mut self, id: SessionId) {
        if let Ok(now) = self.activity.now_ms() {
            let _ = self.activity.persistence.touch(id, now);
        }
    }
    /// When the owner must run the next sweep. None while a close or
    /// shutdown holds metadata; its end wakes the owner.
    pub(crate) fn retention_deadline(&self) -> Option<Instant> {
        if self.stopping || self.metadata_closing || self.retention.deleting.is_some() {
            return None;
        }
        let at = self.retention.next_ms()?;
        Some(
            self.activity
                .owner
                .clock()
                .instant_at(at)
                .unwrap_or_else(Instant::now),
        )
    }
    pub(crate) fn pump_retention(&mut self) {
        if let Ok(now) = self.activity.now_ms() {
            self.pump_retention_at(now);
        }
    }
    pub(crate) fn pump_retention_at(&mut self, now_ms: u64) {
        if self.stopping || self.metadata_closing || self.ownership_lost.is_cancelled() {
            return;
        }
        if let Some(deleting) = &self.retention.deleting {
            match deleting.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    eprintln!("Automatic session deletion failed; it will retry: {error:#}");
                    self.retention.retry_later(now_ms);
                }
                Err(TryRecvError::Disconnected) => self.retention.retry_later(now_ms),
            }
            self.retention.deleting = None;
        }
        let revision = self.host.revision();
        if self.retention.revision != Some(revision) {
            self.retention.revision = Some(revision);
            self.retention.due_ms = self.retention_inputs().and_then(|(snapshot, settings)| {
                next_due(&snapshot.sessions, &settings, &self.activity.persistence)
            });
        }
        if self.retention.next_ms().is_some_and(|at| now_ms >= at) {
            self.sweep(now_ms);
        }
    }
    fn retention_inputs(&self) -> Option<(Snapshot, Settings)> {
        Some((
            self.host.snapshot().ok()?,
            self.host.global_settings().ok()?,
        ))
    }
    fn sweep(&mut self, now_ms: u64) {
        let Some((snapshot, settings)) = self.retention_inputs() else {
            return self.retention.retry_later(now_ms);
        };
        let due = expiries(
            &snapshot.sessions,
            &settings,
            &self.activity.persistence,
            now_ms,
        );
        let identities: Vec<Identity> = self
            .terminals
            .values()
            .map(|terminal| terminal.runtime.identity())
            .filter(|identity| due.iter().any(|(id, _)| *id == identity.session_id))
            .collect();
        if !identities.is_empty() && !self.expiry_evidence(&identities, now_ms) {
            return;
        }
        self.retention.refresh = None;
        // Activity seen by the refresh can delay an expiry, so decide again.
        let decided = expiries(
            &snapshot.sessions,
            &settings,
            &self.activity.persistence,
            now_ms,
        );
        for (id, action) in decided {
            if !due.iter().any(|(due, _)| *due == id) {
                continue;
            }
            match action {
                ExpiryAction::Settle => {
                    if let Err(error) = self.host.settle_idle_session(id) {
                        eprintln!("Automatic session settling failed; it will retry: {error}");
                        self.retention.retry_later(now_ms);
                    }
                }
                // One deletion at a time; the rest follow once it commits.
                ExpiryAction::Delete if self.retention.deleting.is_none() => {
                    let (send, receive) = mpsc::channel();
                    self.retention.deleting = Some(receive);
                    self.begin_close(
                        CloseTarget::Session(id),
                        Box::new(move |result| {
                            let _ = send.send(result);
                        }),
                    );
                }
                _ => {}
            }
        }
        let next = self.retention_inputs().and_then(|(snapshot, settings)| {
            next_due(&snapshot.sessions, &settings, &self.activity.persistence)
        });
        self.retention.revision = Some(self.host.revision());
        self.retention.due_ms = next;
        if self.retention.deleting.is_none() && next.is_some_and(|at| at <= now_ms) {
            self.retention.retry_later(now_ms);
        }
    }
    /// Refreshes every live tab of the due sessions, hidden ones included, so
    /// silent child processes count as activity. True once all have fresh,
    /// healthy evidence; until then the owner rechecks shortly, up to the limit.
    fn expiry_evidence(&mut self, identities: &[Identity], now_ms: u64) -> bool {
        let refresh = *self.retention.refresh.get_or_insert(Refresh {
            since_ms: now_ms,
            next_ms: now_ms,
            until_ms: now_ms.saturating_add(REFRESH_LIMIT_MS),
        });
        self.activity
            .owner
            .begin_expiry_refresh(identities, refresh.since_ms);
        let updates = self.activity.owner.poll_processes(
            &self.terminals,
            &self.activity.rules,
            now_ms,
            identities.len(),
            true,
        );
        self.activity.dirty |= !updates.is_empty();
        for update in updates.iter().filter(|update| update.touch_session) {
            let _ = self
                .activity
                .persistence
                .touch(update.identity.session_id, now_ms);
        }
        if self
            .activity
            .owner
            .fresh_healthy(identities, refresh.since_ms)
        {
            return true;
        }
        if now_ms >= refresh.until_ms {
            self.retention.retry_later(now_ms);
        } else {
            self.retention.refresh = Some(Refresh {
                next_ms: now_ms.saturating_add(REFRESH_STEP_MS),
                ..refresh
            });
        }
        false
    }
}
