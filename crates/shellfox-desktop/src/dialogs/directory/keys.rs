use super::*;
use gpui::{KeyDownEvent, KeyUpEvent};

impl Root {
    pub(crate) fn directory_key_up(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.dialogs.current, Some(Dialog::Create(_)))
            && matches!(event.keystroke.key.as_str(), "enter" | "space")
        {
            // The key-down can move focus to Keep editing before key-up.
            // Don't let GPUI click the newly focused button on release.
            window.prevent_default();
            cx.stop_propagation();
        }
    }

    pub(crate) fn directory_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Dialog::Create(draft)) = &mut self.dialogs.current else {
            return;
        };
        if draft.busy.is_some() || self.dialogs.user_action_pending || draft.path.read(cx).is_composing() {
            return;
        }
        let input = draft.path.read(cx).edit_focus().is_focused(window);
        let shift = event.keystroke.modifiers.shift;
        let key = event.keystroke.key.as_str();
        if event.is_held && (key == "enter" || key == "space" && !input) {
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        match key {
            "escape" => {
                if input && !draft.matches(cx).is_empty() {
                    draft.completion.dismiss();
                } else {
                    self.dismiss_directory(window, cx);
                }
            }
            "tab" if input && shift => {
                if draft.target.is_some() {
                    draft.close.focus(window);
                } else {
                    draft.browse.focus(window);
                }
            }
            "tab" if input && draft.missing.is_some() => {}
            "tab" if input => {
                if draft.completion.dismissed {
                    draft.completion.reset();
                }
                if let Some(value) = draft.completion.tab() {
                    Self::accept_directory_match(draft, value, cx);
                } else {
                    self.query_directory_matches(true, cx);
                }
            }
            "tab" if draft.browse.is_focused(window) => {
                if shift {
                    draft.path.read(cx).focus(window);
                } else if draft.missing.is_some() {
                    draft.keep_editing.focus(window);
                } else {
                    draft.close.focus(window);
                }
            }
            "tab" if draft.keep_editing.is_focused(window) => {
                if shift {
                    if draft.target.is_some() {
                        draft.path.read(cx).focus(window);
                    } else {
                        draft.browse.focus(window);
                    }
                } else {
                    draft.create_missing.focus(window);
                }
            }
            "tab" if draft.create_missing.is_focused(window) => {
                if shift {
                    draft.keep_editing.focus(window);
                } else {
                    draft.close.focus(window);
                }
            }
            "tab" => {
                if !shift {
                    draft.path.read(cx).focus(window);
                } else if draft.missing.is_some() {
                    draft.create_missing.focus(window);
                } else if draft.target.is_some() {
                    draft.path.read(cx).focus(window);
                } else {
                    draft.browse.focus(window);
                }
            }
            "enter" | "space" if draft.close.is_focused(window) => {
                self.dismiss_directory(window, cx)
            }
            "up" | "down" if input => {
                if !draft.completion.move_active(&draft.seen.0, key == "up") {
                    return;
                }
                draft.scroll.scroll_to_item(draft.active());
            }
            "enter" | "space" if draft.browse.is_focused(window) => {
                self.browse_directory(window, cx)
            }
            "enter" | "space" if draft.keep_editing.is_focused(window) => {
                self.dismiss_directory(window, cx)
            }
            "enter" | "space" if draft.create_missing.is_focused(window) => {
                self.submit_directory(Some(Busy::Creating), None, window, cx)
            }
            "enter" if input => {
                if !self.choose_directory_match(None, cx) {
                    self.submit_directory(None, None, window, cx);
                }
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn dismiss_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Create(draft)) = &mut self.dialogs.current else {
            return;
        };
        if self.dialogs.user_action_pending || draft.busy.is_some() {
            return;
        }
        if draft.missing.take().is_some() {
            draft.error = None;
            draft.path.read(cx).focus(window);
            cx.notify();
        } else {
            let prior = draft.prior.clone();
            self.dialogs.current = None;
            if let Some(prior) = prior {
                prior.focus(window);
            } else {
                self.focus_selected(window, cx);
            }
            cx.notify();
        }
    }
}
