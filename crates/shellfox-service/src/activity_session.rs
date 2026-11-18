use shellfox_core::Settings;

pub const LIFECYCLE_INTERVAL_MS: u64 = 60_000;
const HOUR_MS: f64 = 3_600_000.0;
#[derive(Clone, Copy, Debug)]
pub struct SessionAging {
    pub updated_ms: u64,
    pub settled_ms: Option<u64>,
    pub pinned: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpiryAction {
    None,
    Settle,
    Delete,
}
/// One per session. The host persists touches once per minute or at shutdown.
#[derive(Default)]
pub struct SessionActivity {
    pending_ms: Option<u64>,
}
impl SessionActivity {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn touch(&mut self, now_ms: u64) {
        self.pending_ms = Some(
            self.pending_ms
                .map_or(now_ms, |previous| previous.max(now_ms)),
        );
    }
    pub fn last_action_ms(&self, session: SessionAging) -> u64 {
        session.updated_ms.max(self.pending_ms.unwrap_or(0))
    }
    /// Keep the pending value until persistence succeeds. Newer touches survive.
    pub fn pending_update(&self, persisted_ms: u64) -> Option<u64> {
        self.pending_ms.filter(|at| *at > persisted_ms)
    }
    pub fn persisted(&mut self, through_ms: u64) {
        if self.pending_ms.is_some_and(|at| at <= through_ms) {
            self.pending_ms = None;
        }
    }
    pub fn due(&self, session: SessionAging, settings: &Settings, now_ms: u64) -> bool {
        let last = self.last_action_ms(session);
        let hours = match session.settled_ms {
            Some(_) if session.pinned => return false,
            Some(_) => settings.session_delete_hours,
            None => settings.session_settle_hours,
        };
        if !hours.is_finite() || hours < 0.0 {
            return false;
        }
        let base = session.settled_ms.map_or(last, |settled| settled.max(last));
        // JavaScript Date's supported range is exactly representable as f64.
        if now_ms > 8_640_000_000_000_000 || base > 8_640_000_000_000_000 {
            return false;
        }
        (now_ms as f64) >= (base as f64) + hours * HOUR_MS
    }
    /// The caller refreshes ALL owned roots before this decision, including hidden
    /// sessions, then rechecks touches received while polling. Fresh healthy proof
    /// must cover every still-owned generation. Unknown/stale evidence fails closed.
    /// Settle starts a NEW deletion grace period; deletion must use normal owned close.
    pub fn expiry(
        &self,
        session: SessionAging,
        settings: &Settings,
        now_ms: u64,
        fresh_healthy: bool,
    ) -> ExpiryAction {
        if !fresh_healthy || !self.due(session, settings, now_ms) {
            ExpiryAction::None
        } else if session.settled_ms.is_some() {
            ExpiryAction::Delete
        } else {
            ExpiryAction::Settle
        }
    }
}
/// Distinct manual settlements need distinct captured-input markers, even when the
/// wall clock is fixed or moves backwards. The host serializes/persists the token.
#[derive(Default)]
pub struct SettlementClock {
    last_ms: u64,
}
impl SettlementClock {
    pub fn seed(&mut self, settled_ms: u64) {
        self.last_ms = self.last_ms.max(settled_ms);
    }
    pub fn next(&mut self, now_ms: u64) -> Option<u64> {
        let next = now_ms.max(self.last_ms.checked_add(1)?);
        self.last_ms = next;
        Some(next)
    }
}
