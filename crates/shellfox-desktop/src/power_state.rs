use shellfox_service::power::{PowerReport, PowerStatus};
use std::time::Instant;
#[derive(Default)]
pub(crate) struct PowerUi {
    pub report: Option<PowerReport>,
    pub error: Option<String>,
    pub request: u64,
    pub inflight: bool,
    pub changing: bool,
    pub last_read: Option<Instant>,
}
impl PowerUi {
    pub fn held(&self) -> bool {
        self.report
            .as_ref()
            .is_some_and(|r| r.status != PowerStatus::Inactive)
    }
    pub fn blocked(&self) -> bool {
        self.inflight || self.report.as_ref().is_none_or(|r| r.pending)
    }
    pub fn blocks_close(&self) -> bool {
        self.changing || self.report.as_ref().is_some_and(|r| r.pending)
    }
    pub fn toggled(&self) -> Option<accesskit::Toggled> {
        self.report.as_ref().map(|r| match r.status {
            PowerStatus::Inactive => accesskit::Toggled::False,
            PowerStatus::Active => accesskit::Toggled::True,
            PowerStatus::Partial | PowerStatus::ReleaseFailed => accesskit::Toggled::Mixed,
        })
    }
    pub fn description(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        let Some(report) = &self.report else {
            return "Local host keep-awake status is not confirmed.".into();
        };
        if let Some(error) = &report.error {
            return error.clone();
        }
        if let Some(reason) = &report.partial_reason {
            return reason.clone();
        }
        if report.pending {
            return "Local host keep-awake change is pending.".into();
        }
        match report.status {
            PowerStatus::Inactive => "Keep awake is off for the local host.",
            PowerStatus::Active => "Keep awake is active for the local host.",
            PowerStatus::Partial => "Keep awake is partially active for the local host.",
            PowerStatus::ReleaseFailed => {
                "Local host release failed; retry turning keep awake off."
            }
        }
        .into()
    }
    pub fn begin(&mut self, changing: bool) -> Option<u64> {
        if self.inflight {
            return None;
        }
        self.request = self.request.checked_add(1)?;
        self.inflight = true;
        self.changing = changing;
        self.last_read = Some(Instant::now());
        Some(self.request)
    }
    pub fn complete(&mut self, id: u64, result: Result<PowerReport, String>) -> bool {
        if id != self.request || !self.inflight {
            return false;
        }
        self.inflight = false;
        self.changing = false;
        match result {
            Ok(report) => {
                self.report = Some(report);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        true
    }
}
pub(super) fn confirmed_target(report: &PowerReport) -> Option<bool> {
    if report.pending {
        None
    } else {
        Some(report.status == PowerStatus::Inactive)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn report(status: PowerStatus) -> PowerReport {
        PowerReport {
            status,
            partial_reason: (status == PowerStatus::Partial)
                .then(|| "lid inhibition unavailable".into()),
            error: (status == PowerStatus::ReleaseFailed).then(|| "release unconfirmed".into()),
            pending: false,
        }
    }
    #[test]
    fn partial_and_failed_release_keep_held_state_and_only_retry_off() {
        for status in [
            PowerStatus::Active,
            PowerStatus::Partial,
            PowerStatus::ReleaseFailed,
        ] {
            let value = report(status);
            assert_eq!(confirmed_target(&value), Some(false));
            let state = PowerUi {
                report: Some(value),
                ..Default::default()
            };
            assert!(state.held());
            if status != PowerStatus::Active {
                assert_eq!(state.toggled(), Some(accesskit::Toggled::Mixed));
            }
        }
        assert_eq!(confirmed_target(&report(PowerStatus::Inactive)), Some(true));
    }
    #[test]
    fn pending_errors_and_stale_replies_never_optimistically_flip_confirmation() {
        let mut state = PowerUi {
            report: Some(report(PowerStatus::Active)),
            ..Default::default()
        };
        let id = state.begin(true).unwrap();
        assert!(state.blocked() && state.blocks_close() && state.held());
        assert!(!state.complete(id + 1, Ok(report(PowerStatus::Inactive))));
        assert!(state.inflight);
        assert!(state.complete(id, Err("unconfirmed release".into())));
        assert!(state.held());
        assert_eq!(state.error.as_deref(), Some("unconfirmed release"));
        let mut pending = report(PowerStatus::Inactive);
        pending.pending = true;
        assert_eq!(confirmed_target(&pending), None);
    }
}
