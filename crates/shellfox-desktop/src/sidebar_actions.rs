use super::*;
use crate::workspace::AfterAction;
use gpui::Window;
use shellfox_service::native::NativeAction;

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
        self.submit_native(action, after, window, cx);
    }
}
