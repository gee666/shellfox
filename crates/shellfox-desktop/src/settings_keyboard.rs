use super::*;
impl SettingsPanel {
    pub(super) fn key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.frozen() {
            return;
        }
        let key = event.keystroke.key.as_str();
        if self
            .environment
            .as_ref()
            .is_some_and(|env| env.text.read(cx).is_composing())
        {
            return;
        }
        if key == "escape" {
            if self.environment.is_some() {
                self.environment = None;
                cx.notify();
            } else if self.profile_menu {
                self.profile_menu = false;
                cx.notify();
            } else if self.ssh.editor.is_some() || self.ssh.removing.is_some() {
                self.ssh.cancel_editor();
                cx.notify();
            } else {
                self.request_close(cx);
            }
            window.prevent_default();
            cx.stop_propagation();
        } else if key == "enter"
            && event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && self.environment.is_some()
        {
            self.save_environment(cx);
            window.prevent_default();
            cx.stop_propagation();
        }
    }
}
