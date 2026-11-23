use super::*;
impl SettingsPanel {
    pub(crate) fn blocks_layout_shortcuts(&self) -> bool {
        self.environment.is_some() || self.color_picker.is_some() || self.profile_menu
    }

    pub(super) fn key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.frozen() || self.color_picker.is_some() {
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
        if key == "tab" && self.environment.is_none() && !self.profile_menu {
            self.keyboard_focus = true;
            if event.keystroke.modifiers.shift {
                window.focus_prev();
            } else {
                window.focus_next();
            }
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        } else if key == "escape" {
            if self.environment.is_some() {
                self.close_environment(window, cx);
            } else if self.profile_menu {
                self.profile_menu = false;
                cx.notify();
            } else if self.category == Category::Ssh
                && (self.ssh.editor.is_some() || self.ssh.removing.is_some())
            {
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
