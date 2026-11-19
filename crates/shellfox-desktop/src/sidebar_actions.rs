use super::*;
use crate::workspace::AfterAction;
use gpui::Window;
use shellfox_service::native::NativeAction;

pub(super) struct DeferredAction {
    action: NativeAction,
    after: AfterAction,
    source_epoch: u64,
}

impl Root {
    pub(crate) fn submit_sidebar_action(
        &mut self,
        action: NativeAction,
        after: AfterAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window || self.user_action_pending {
            return;
        }
        if self.action_pending {
            // A background read isn't a user action. Queue once before native
            // admission; a dispatched mutation is never reissued.
            self.sidebar_ui.deferred_action = Some(DeferredAction {
                action,
                after,
                source_epoch: self.source_selection_epoch(),
            });
            self.user_action_pending = true;
            cx.notify();
        } else {
            self.submit_native(action, after, window, cx);
        }
    }

    pub(crate) fn flush_sidebar_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.action_pending {
            return;
        }
        let Some(action) = self.sidebar_ui.deferred_action.take() else {
            return;
        };
        if !self.closing_window && self.source_selection_epoch() == action.source_epoch {
            self.submit_native(action.action, action.after, window, cx);
        }
    }
}
