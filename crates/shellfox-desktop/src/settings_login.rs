use super::api::{NetworkStatus, RemoteState};
/// Known frontend blockers only. The owner still validates live resources and
/// cleanup before admitting Login; a false value is not a readiness proof.
pub(super) fn blocked(state: &RemoteState) -> bool {
    !state.enabled
        || matches!(
            state.status,
            NetworkStatus::Disabled | NetworkStatus::Online
        )
        || state.attempts.iter().any(|attempt| {
            matches!(
                attempt.kind,
                super::api::RemoteAttemptKind::Enable
                    | super::api::RemoteAttemptKind::Resume
                    | super::api::RemoteAttemptKind::Login
            )
        })
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    Requesting,
    Browser,
    Error,
    Cancelled,
}
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Browser,
    Key,
}
struct Attempt {
    serial: u64,
    baseline: Option<String>,
    latest: Option<String>,
    kind: Kind,
    opened: bool,
}
pub(super) struct Login {
    pub phase: Phase,
    pub message: String,
    pub url: Option<String>,
    serial: u64,
    attempt: Option<Attempt>,
}
impl Login {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            message: String::new(),
            url: None,
            serial: 0,
            attempt: None,
        }
    }
    pub fn serial(&self) -> u64 {
        self.serial
    }
    pub fn is_current(&self, serial: u64) -> bool {
        self.attempt
            .as_ref()
            .is_some_and(|attempt| attempt.serial == serial)
    }
    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Requesting | Phase::Browser)
    }
    pub fn needs_release(&self) -> bool {
        matches!(self.phase, Phase::Error | Phase::Cancelled) && self.attempt.is_none()
    }
    pub fn dismissed(&mut self) {
        self.idle();
    }
    pub fn start(&mut self, state: &RemoteState, kind: Kind) -> u64 {
        self.serial += 1;
        self.attempt = Some(Attempt {
            serial: self.serial,
            baseline: state.login_url.clone(),
            latest: None,
            kind,
            opened: false,
        });
        self.phase = Phase::Requesting;
        self.url = None;
        self.message = match kind {
            Kind::Browser => "Starting sign-in…",
            Kind::Key => "Connecting with auth key…",
        }
        .into();
        self.serial
    }
    fn idle(&mut self) {
        self.attempt = None;
        self.phase = Phase::Idle;
        self.url = None;
        self.message.clear();
    }
    /// True requests one automatic browser open. A later pushed URL never opens
    /// another browser window or lets an older login reply replace that address.
    pub fn accept(&mut self, state: &RemoteState, confirmed: bool) -> bool {
        if state.status == NetworkStatus::Online
            || !state.enabled
            || state.status == NetworkStatus::Disabled
        {
            self.idle();
            return false;
        }
        let Some(attempt) = &mut self.attempt else {
            return false;
        };
        if state.status == NetworkStatus::Error {
            self.phase = Phase::Error;
            self.message = state
                .error
                .clone()
                .unwrap_or_else(|| "Sign-in failed. Try again.".into());
            self.url = attempt.latest.clone();
            self.attempt = None;
            return false;
        }
        let Some(url) = &state.login_url else {
            attempt.baseline = None;
            return false;
        };
        if confirmed && (attempt.opened || attempt.latest.is_some()) {
            return false;
        }
        if matches!(attempt.kind, Kind::Key) {
            attempt.latest = Some(url.clone());
            self.url = Some(url.clone());
            return false;
        }
        if state.status != NetworkStatus::LoginRequired
            || (!confirmed && Some(url) == attempt.baseline.as_ref())
        {
            return false;
        }
        attempt.latest = Some(url.clone());
        self.url = Some(url.clone());
        if attempt.opened {
            self.message = "Sign-in address updated. Continue in browser.".into();
            return false;
        }
        attempt.opened = true;
        self.phase = Phase::Browser;
        self.message = "Finish signing in in your browser. This page updates automatically.".into();
        true
    }
    pub fn cancel(&mut self) {
        self.serial += 1;
        self.attempt = None;
        self.phase = Phase::Cancelled;
        self.url = None;
        self.message = "Sign-in cancelled.".into();
    }
    pub fn fail(&mut self) {
        if !self.busy() {
            return;
        }
        self.attempt = None;
        self.phase = Phase::Error;
        self.message = "Could not start sign-in. Check your connection and try again.".into();
    }
    pub fn browser_failed(&mut self) {
        self.phase = Phase::Error;
        self.message =
            "Could not open your browser. Continue sign-in or copy the login address.".into();
    }
    pub fn continue_browser(&mut self, state: &RemoteState) -> Option<u64> {
        let url = self
            .attempt
            .as_ref()
            .and_then(|attempt| attempt.latest.clone())
            .or_else(|| self.url.clone())
            .or_else(|| {
                if state.status == NetworkStatus::LoginRequired {
                    state.login_url.clone()
                } else {
                    None
                }
            })?;
        if self.attempt.is_none() {
            self.serial += 1;
            self.attempt = Some(Attempt {
                serial: self.serial,
                baseline: Some(url.clone()),
                latest: Some(url.clone()),
                kind: Kind::Browser,
                opened: true,
            });
        }
        self.url = Some(url);
        self.phase = Phase::Browser;
        self.message = "Finish signing in in your browser. This page updates automatically.".into();
        Some(self.serial)
    }
    pub fn timeout(&mut self, serial: u64) {
        if self
            .attempt
            .as_ref()
            .is_some_and(|attempt| attempt.serial == serial)
        {
            self.attempt = None;
            self.phase = Phase::Error;
            self.message = "Sign-in timed out. Try again or continue in your browser.".into();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_browser_open_keeps_attempt_but_expiration_enables_retry() {
        let mut login = Login::new();
        let state = state(Some("https://login.tailscale.com/p5-fixture"));
        let serial = login.start(&state, Kind::Browser);
        assert!(login.accept(&state, true));
        login.browser_failed();
        assert!(!login.needs_release());
        login.timeout(serial);
        assert!(login.needs_release());
    }

    #[test]
    fn cancel_before_login_response_survives_a_late_failure() {
        let mut login = Login::new();
        let state = state(Some("https://login.tailscale.com/fixture"));
        login.start(&state, Kind::Browser);
        login.cancel();
        assert!(!login.accept(&state, true));
        login.fail();
        assert!(login.phase == Phase::Cancelled && login.needs_release());
        login.dismissed();
        assert!(login.phase == Phase::Idle && !login.needs_release());
    }

    fn state(url: Option<&str>) -> RemoteState {
        RemoteState {
            enabled: true,
            hostname: "fixture".into(),
            status: NetworkStatus::LoginRequired,
            login_url: url.map(str::to_owned),
            error: None,
            peers: vec![],
            pending: vec![],
            approvals: vec![],
            attempts: vec![],
            limitations: vec![],
        }
    }
    #[test]
    fn actual_pending_enable_resume_and_login_block_duplicate_login_intents() {
        let mut current = state(None);
        assert!(!blocked(&current));
        for kind in [
            super::super::api::RemoteAttemptKind::Resume,
            super::super::api::RemoteAttemptKind::Enable,
            super::super::api::RemoteAttemptKind::Login,
        ] {
            current.attempts = vec![super::super::api::RemoteAttempt {
                id: "00000000-0000-4000-8000-000000000001".into(),
                kind,
            }];
            assert!(blocked(&current));
        }
        current.attempts.clear();
        current.status = NetworkStatus::Disabled;
        assert!(blocked(&current));
        current.status = NetworkStatus::Online;
        assert!(blocked(&current));
    }
    #[test]
    fn opens_once_and_confirmed_old_reply_cannot_replace_new_url() {
        let mut login = Login::new();
        let mut current = state(Some("https://login.tailscale.com/old"));
        login.start(&current, Kind::Browser);
        assert!(!login.accept(&current, false));
        assert!(login.accept(&current, true));
        current.login_url = Some("https://login.tailscale.com/new".into());
        assert!(!login.accept(&current, false));
        assert!(!login.accept(&state(Some("https://login.tailscale.com/old")), true));
        assert_eq!(
            login.url.as_deref(),
            Some("https://login.tailscale.com/new")
        );
    }
    #[test]
    fn auth_keys_never_automatically_open_browser_and_stale_timeout_is_ignored() {
        let mut login = Login::new();
        let current = state(Some("https://login.tailscale.com/address"));
        let old = login.start(&current, Kind::Key);
        assert!(!login.accept(&current, true));
        login.cancel();
        let newer = login.start(&current, Kind::Browser);
        login.timeout(old);
        assert_eq!(login.phase as u8, Phase::Requesting as u8);
        login.timeout(newer);
        assert_eq!(login.phase as u8, Phase::Error as u8);
    }
}
