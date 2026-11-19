use super::*;
impl SettingsPanel {
    pub(super) fn environment_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.frozen() {
            return;
        }
        let Some(environment) = &self.environment else {
            return;
        };
        if environment.text.read(cx).is_composing() {
            return;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.close_environment(window, cx);
        } else if key == "enter"
            && event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
        {
            self.save_environment(cx);
        } else if key == "tab" {
            self.keyboard_focus = true;
            let close = self.controls.get("close-environment");
            let cancel = self.controls.get("cancel-environment");
            let save = self.controls.get("save-environment");
            let current = if close.is_some_and(|focus| focus.is_focused(window)) {
                0
            } else if cancel.is_some_and(|focus| focus.is_focused(window)) {
                2
            } else if save.is_some_and(|focus| focus.is_focused(window)) {
                3
            } else {
                1
            };
            let (_, issues) = environment_data::parse(
                &environment.text.read(cx).text,
                self.catalog
                    .as_ref()
                    .is_none_or(|catalog| catalog.platform == "win32"),
            );
            let disabled = environment.pending || !issues.is_empty() || self.catalog.is_none();
            let mut target = if event.keystroke.modifiers.shift {
                (current + 3) % 4
            } else {
                (current + 1) % 4
            };
            if target == 3 && disabled {
                target = if event.keystroke.modifiers.shift {
                    2
                } else {
                    0
                };
            }
            match target {
                1 => environment.text.read(cx).focus(window),
                0 => {
                    if let Some(focus) = close {
                        focus.focus(window);
                    }
                }
                2 => {
                    if let Some(focus) = cancel {
                        focus.focus(window);
                    }
                }
                _ => {
                    if let Some(focus) = save {
                        focus.focus(window);
                    }
                }
            }
            cx.notify();
        } else {
            return;
        }
        window.prevent_default();
        cx.stop_propagation();
    }
    pub(super) fn close_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        self.environment = None;
        self.environment_subscription = None;
        if let Some(focus) = self.controls.get("all-terminal-environment") {
            focus.focus(window);
        }
        cx.notify();
    }
}
