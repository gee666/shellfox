//! Last opener wins, as in 0.9: showing a terminal takes its control, and a
//! shown terminal takes it back once the view that held it is gone.
use super::*;
use crate::terminal::TerminalRecovery;

/// Why the selected terminal is looked at again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The user showed it: window activation or tab selection.
    Shown,
    /// The host reported that no view controls it, or this view just lost it.
    Freed,
}

/// The attach a blocked, shown terminal makes: a takeover when the user showed
/// it, a plain reattach once its control is free, otherwise none.
fn claim_recovery(blocked: bool, claim: Claim, free: bool) -> Option<TerminalRecovery> {
    match (blocked, claim) {
        (false, _) => None,
        (true, Claim::Shown) => Some(TerminalRecovery::TakeControl),
        (true, Claim::Freed) => free.then_some(TerminalRecovery::Retry),
    }
}

impl Root {
    pub(crate) fn watch_terminal_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.observe_window_activation(window, |root, window, cx| {
            if window.is_window_active() {
                root.claim_selected(Claim::Shown, window, cx);
            }
        })
        .detach();
    }

    pub(crate) fn claim_selected(
        &mut self,
        claim: Claim,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.close.closing || !self.is_local_source() {
            return;
        }
        let Some(id) = self.selected_tab() else {
            return;
        };
        let free = self.tab_control_free(id);
        if let Some(recovery) = claim_recovery(self.control_blocked(id, cx), claim, free) {
            self.attach_local_view(id, Some(recovery), window, cx);
        }
    }

    /// Another view controls this tab: its view lost control, or its attach was refused.
    pub(crate) fn control_blocked(&self, id: TabId, cx: &gpui::App) -> bool {
        match self.tabs.views.get(&id) {
            Some(view) => view.read(cx).control_blocked(),
            None => self
                .tabs
                .failures
                .get(&id)
                .is_some_and(|failure| failure.control_required),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shown_blocked_terminal_takes_control_at_once() {
        assert_eq!(
            claim_recovery(true, Claim::Shown, false),
            Some(TerminalRecovery::TakeControl)
        );
    }

    #[test]
    fn a_shown_blocked_terminal_reattaches_when_its_controller_leaves() {
        assert_eq!(
            claim_recovery(true, Claim::Freed, true),
            Some(TerminalRecovery::Retry)
        );
        // Still controlled elsewhere: wait for the host to report it free.
        assert_eq!(claim_recovery(true, Claim::Freed, false), None);
    }

    #[test]
    fn a_terminal_this_view_controls_needs_no_claim() {
        assert_eq!(claim_recovery(false, Claim::Shown, true), None);
        assert_eq!(claim_recovery(false, Claim::Freed, true), None);
    }
}
