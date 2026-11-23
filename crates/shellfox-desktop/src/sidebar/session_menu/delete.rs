use super::*;
use crate::dialogs::dialog_button;

impl Root {
    pub(crate) fn session_delete_confirmation(
        &self,
        session: &Session,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let running = session
            .tabs
            .iter()
            .any(|tab| self.tab_close_identity(tab.id).is_some());
        self.inline_session_delete(session.title.clone(), running, palette, cx)
    }

    pub(crate) fn inline_session_delete(
        &self,
        title: String,
        running: bool,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let running = if running {
            " Its running terminals will be closed."
        } else {
            ""
        };
        let body = div()
            .id("inline-delete-confirm")
            .absolute()
            .bottom(px(8.))
            .left(px(8.))
            .right(px(8.))
            .occlude()
            .p(px(12.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .rounded(px(6.))
            .shadow_lg()
            .text_size(px(12.))
            .on_key_down(cx.listener(move |root, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => root.cancel_session_delete(window, cx),
                    "tab" if root.sidebar.delete_cancel.is_focused(window) => {
                        root.dialogs.modal_focus.focus(window)
                    }
                    "tab" => root.sidebar.delete_cancel.focus(window),
                    _ => return,
                }
                window.prevent_default();
                cx.stop_propagation();
            }))
            .child(div().mb(px(8.)).child(format!(
                "Permanently delete “{title}”?{running} This cannot be undone.",
            )))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(4.))
                    .child(
                        dialog_button("delete-session-confirm", "Delete", palette)
                            .track_focus(&self.dialogs.modal_focus)
                            .when(self.dialog_locked(), |style| style.opacity(0.4))
                            .on_click(
                                cx.listener(|root, _, window, cx| root.confirm_dialog(window, cx)),
                            ),
                    )
                    .child(
                        dialog_button("delete-session-cancel", "Cancel", palette)
                            .track_focus(&self.sidebar.delete_cancel)
                            .on_click(cx.listener(move |root, _, window, cx| {
                                root.cancel_session_delete(window, cx)
                            })),
                    ),
            );
        self.semantic_region(
            "root/dialog",
            crate::accessibility::Role::Dialog,
            "Delete session",
            body,
        )
        .into_any_element()
    }

    fn cancel_session_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_locked() {
            return;
        }
        if let Some(Dialog::DeleteSession(id)) = self.dialogs.current {
            self.dialogs.current = None;
            self.sidebar.focus_row(id, window);
            cx.notify();
        } else {
            self.dismiss_dialog(window, cx);
        }
    }
}
