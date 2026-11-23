use crate::{
    accessibility::{self, Action, Role, SemanticNode},
    app_host::CloseDecision,
    dialogs::dialog_button,
    dialogs::Dialog,
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, App, Context, Div, FocusHandle, Stateful, Window, div, prelude::*, px};

#[derive(Clone, Copy)]
enum QuitControl {
    Close,
    Cancel,
    Confirm,
}
impl QuitControl {
    const ORDER: [Self; 3] = [Self::Close, Self::Cancel, Self::Confirm];
    fn key(self, epoch: u64) -> String {
        let name = match self {
            Self::Close => "close",
            Self::Cancel => "cancel",
            Self::Confirm => "confirm",
        };
        format!("root/dialog/quit/{epoch}/{name}")
    }
}

pub(crate) struct QuitFocus {
    pub(crate) header: FocusHandle,
    cancel: FocusHandle,
    confirm: FocusHandle,
    previous: Option<FocusHandle>,
    epoch: u64,
    pub(crate) error: Option<String>,
}
impl QuitFocus {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            header: cx.focus_handle(),
            cancel: cx.focus_handle(),
            confirm: cx.focus_handle(),
            previous: None,
            epoch: 0,
            error: None,
        }
    }
    fn handle(&self, control: QuitControl) -> &FocusHandle {
        match control {
            QuitControl::Close => &self.header,
            QuitControl::Cancel => &self.cancel,
            QuitControl::Confirm => &self.confirm,
        }
    }

    pub(crate) fn restore(&mut self, window: &mut Window) -> bool {
        if let Some(previous) = self.previous.take() {
            previous.focus(window);
            true
        } else {
            false
        }
    }
}

impl Root {
    pub(crate) fn open_quit_dialog(&mut self, window: &mut Window, cx: &Context<Self>) {
        window.activate_window();
        self.close.quit_focus.error = None;
        self.close.quit_focus.previous = window.focused(cx);
        self.close.quit_focus.epoch = self.close.quit_focus.epoch.wrapping_add(1);
        self.close.quit_focus.cancel.focus(window);
        self.dialogs.current = Some(Dialog::Quit);
    }

    pub(crate) fn quit_admission_failed(
        &mut self,
        error: anyhow::Error,
        decision: Option<CloseDecision>,
        restart_request: Option<crate::update::RestartRequest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close.confirmation = None;
        self.close.ticket = None;
        match decision {
            Some(CloseDecision::Ready(ticket)) => self.close.ticket = Some(ticket),
            Some(CloseDecision::Confirm(confirmation)) => {
                self.close.confirmation = Some(confirmation)
            }
            _ => {}
        }
        self.update_ui.restart_request = restart_request;
        self.close.quit_focus.error = Some(error.to_string());
        self.close.quit_focus.epoch = self.close.quit_focus.epoch.wrapping_add(1);
        self.close.quit_focus.cancel.focus(window);
        cx.notify();
    }

    fn quit_has_approval(&self) -> bool {
        self.close.confirmation.is_some() || self.close.ticket.is_some()
    }

    pub(crate) fn quit_dialog_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = &self.close.quit_focus;
        let order = QuitControl::ORDER.map(|control| focus.handle(control));
        let order = &order[..if self.quit_has_approval() { 3 } else { 2 }];
        let index = order
            .iter()
            .position(|handle| handle.is_focused(window))
            .unwrap_or(1);
        match event.keystroke.key.as_str() {
            "escape" => self.dismiss_dialog(window, cx),
            "tab" => {
                let step = if event.keystroke.modifiers.shift {
                    order.len() - 1
                } else {
                    1
                };
                order[(index + step) % order.len()].focus(window);
                cx.notify();
            }
            "enter" | "space" if index == 2 => self.confirm_dialog(window, cx),
            "enter" | "space" => self.dismiss_dialog(window, cx),
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    pub(crate) fn quit_close_button(&self, child: Stateful<Div>) -> AnyElement {
        self.quit_control(QuitControl::Close, "Close dialog", child)
    }

    fn quit_control(
        &self,
        control: QuitControl,
        name: &'static str,
        child: Stateful<Div>,
    ) -> AnyElement {
        let frame = self.semantics.context.frame();
        let ids = frame
            .id_for(&control.key(self.close.quit_focus.epoch))
            .and_then(|id| frame.id_for("root/dialog").map(|parent| (id, parent)));
        match ids {
            Ok((id, parent)) => {
                let mut node = SemanticNode::new(id, Some(parent), Role::Button, name);
                if self.dialog_locked()
                    || matches!(control, QuitControl::Confirm) && !self.quit_has_approval()
                {
                    node.data.set_disabled();
                } else {
                    node.data.add_action(Action::Click);
                    node.focus = Some(self.close.quit_focus.handle(control).clone());
                }
                frame.wrap(node, child).into_any_element()
            }
            Err(error) => {
                frame.reject(error);
                child.into_any_element()
            }
        }
    }

    pub(crate) fn handle_quit_semantic_action(
        &mut self,
        action: &accessibility::SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use accessibility::SemanticActionKind;
        if !matches!(self.dialogs.current, Some(Dialog::Quit)) || self.dialog_locked() {
            return false;
        }
        let frame = self.semantics.context.frame();
        if frame.focus_handle(action.target).is_none() {
            return false;
        }
        let Some(control) = QuitControl::ORDER.into_iter().find(|control| {
            frame.id_for(&control.key(self.close.quit_focus.epoch)).ok() == Some(action.target)
        }) else {
            return false;
        };
        match action.kind {
            SemanticActionKind::Focus => {
                self.close.quit_focus.handle(control).focus(window);
                cx.notify();
            }
            SemanticActionKind::Click => match control {
                QuitControl::Confirm => self.confirm_dialog(window, cx),
                _ => self.dismiss_dialog(window, cx),
            },
            _ => return false,
        }
        true
    }

    pub(crate) fn render_quit_dialog(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let count = self
            .close.confirmation
            .as_ref()
            .map_or(0, |confirmation| confirmation.terminal_count);
        let update = self.close.update_restart;
        let question = match (count, update) {
            (0, true) => "Install the update and restart Shellfox?".into(),
            (0, false) => "Close shells and quit?".into(),
            (count, update) => {
                let plural = if count == 1 { "" } else { "s" };
                let action = if update { "install the update" } else { "quit" };
                format!("Close {count} shell{plural} and {action}?")
            }
        };
        let confirm_label = if update {
            "Close shells and install"
        } else {
            "Close shells and quit"
        };
        let cancel = dialog_button("dialog-cancel", "Cancel", palette)
            .track_focus(&self.close.quit_focus.cancel)
            .on_click(cx.listener(|root, _, window, cx| root.dismiss_dialog(window, cx)));
        let confirm = dialog_button("dialog-confirm", confirm_label, palette)
            .track_focus(&self.close.quit_focus.confirm)
            .on_click(cx.listener(|root, _, window, cx| root.confirm_dialog(window, cx)));
        let locked = self.dialog_locked();
        let active = window.is_window_active() && !locked;
        let cancel = cancel.when(locked, |button| button.opacity(0.45));
        let confirm = confirm.when(locked || !self.quit_has_approval(), |button| {
            button.opacity(0.45)
        });
        let content = div().p(px(12.)).flex().flex_col().gap(px(8.)).text_size(px(12.)).line_height(px(16.))
            .child(question).when(count > 0, |content| content.child(
                div().text_size(px(11.)).line_height(px(16.)).text_color(palette.muted)
                    .child("Running commands will stop. Shells restart fresh next time. External terminals stay open.")
            ))
            .when_some(self.close.quit_focus.error.as_ref(), |content, message| {
                content.child(self.semantic_region("root/dialog/quit-error", Role::Alert, message.clone(),
                    div().text_size(px(11.)).text_color(palette.status_color(0xfc8397)).child(message.clone())))
            })
            .child(div().flex().justify_end().gap(px(6.)).mt(px(8.))
                .child(self.quit_control(QuitControl::Cancel, "Cancel", cancel.when(active, |button| button.focus(|style| style.border_color(palette.accent)))))
                .child(self.quit_control(QuitControl::Confirm, confirm_label, confirm.when(active, |button| button.focus(|style| style.border_color(palette.accent))))));
        self.modal_frame(
            if update {
                "Install update?"
            } else {
                "Quit Shellfox?"
            },
            content.into_any_element(),
            palette,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::QuitControl;

    #[test]
    fn a_confirm_button_from_an_earlier_prompt_cannot_target_the_next_prompt() {
        let earlier = QuitControl::Confirm.key(9);
        assert!(
            QuitControl::ORDER
                .into_iter()
                .all(|control| control.key(10) != earlier)
        );
    }
}
