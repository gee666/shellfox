use crate::{
    assets::icon_button,
    chrome::button,
    field::Field,
    root::{Dialog, Root},
    theme::Palette,
    workspace::AfterAction,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use shellfox_host::{HostRequest, SessionId, TabId};
use shellfox_service::native::NativeAction;

impl Root {
    pub(crate) fn rename_session_dialog(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.user_action_pending || self.closing_window {
            return;
        }
        if let Some(session) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
        {
            let title = Field::new(session.title.clone(), cx);
            title.read(cx).focus(window);
            self.dialog = Some(Dialog::RenameSession { id, title });
        }
        cx.notify();
    }
    pub(crate) fn rename_tab_dialog(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.user_action_pending || self.closing_window {
            return;
        }
        if let Some(tab) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .sessions
                .iter()
                .flat_map(|session| &session.tabs)
                .find(|tab| tab.id == id)
        }) {
            let title = Field::new(tab.title.clone(), cx);
            title.read(cx).focus(window);
            self.dialog = Some(Dialog::RenameTab { id, title });
        }
        cx.notify();
    }
    pub(crate) fn modal_frame(
        &self,
        title: &'static str,
        content: AnyElement,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let black: gpui::Hsla = gpui::rgb(0).into();
        let dialog = div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(black.opacity(0.53))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|root, _, window, cx| root.dismiss_dialog(window, cx)),
            )
            .child(
                self.semantic_region(
                    "root/dialog",
                    crate::accessibility::Role::Dialog,
                    title,
                    div()
                        .track_focus(&self.modal_focus)
                        .on_key_down(cx.listener(|root, event: &gpui::KeyDownEvent, window, cx| {
                            if root.dialog_locked()
                                || matches!(root.dialog, Some(Dialog::Create(_)))
                            {
                                return;
                            }
                            let composing = match &root.dialog {
                                Some(
                                    Dialog::RenameSession { title, .. }
                                    | Dialog::RenameTab { title, .. },
                                ) => title.read(cx).is_composing(),
                                _ => false,
                            };
                            if composing {
                                return;
                            }
                            match event.keystroke.key.as_str() {
                                "escape" => root.dismiss_dialog(window, cx),
                                "enter" => {
                                    if matches!(root.dialog, Some(Dialog::Quit)) {
                                        root.dismiss_dialog(window, cx);
                                    } else {
                                        root.confirm_dialog(window, cx);
                                    }
                                }
                                _ => return,
                            }
                            window.prevent_default();
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .w(px(if title == "Quit Shellfox?" {
                            360.
                        } else {
                            540.
                        }))
                        .max_w_full()
                        .flex()
                        .flex_col()
                        .rounded(px(4.))
                        .bg(palette.panel)
                        .border_1()
                        .border_color(palette.border)
                        .shadow(vec![gpui::BoxShadow {
                            color: black.opacity(0.53),
                            offset: gpui::point(px(0.), px(16.)),
                            blur_radius: px(64.),
                            spread_radius: px(0.),
                        }])
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|_, _, _, cx| cx.stop_propagation()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .pl(px(16.))
                                .pr(px(12.))
                                .py(px(8.))
                                .border_b_1()
                                .border_color(palette.border)
                                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
                                .child(icon_button("close-dialog", "close", palette).on_click(
                                    cx.listener(|root, _, window, cx| {
                                        root.dismiss_dialog(window, cx)
                                    }),
                                )),
                        )
                        .child(content),
                ),
            );
        self.semantic_context
            .frame()
            .overlay_subtree(dialog)
            .into_any_element()
    }
    pub(crate) fn render_dialog(&self, palette: Palette, cx: &Context<Self>) -> AnyElement {
        let Some(dialog) = &self.dialog else {
            return div().into_any_element();
        };
        if let Dialog::Create(draft) = dialog {
            return self.render_directory(draft, palette, cx);
        }
        let mut content = div().p_3().flex().flex_col().gap_2();
        let title = match dialog {
            Dialog::RenameSession { title, .. } | Dialog::RenameTab { title, .. } => {
                content = content.child(title.clone());
                "Rename"
            }
            Dialog::DeleteSession(_) => {
                content = content.child(
                    "Permanently delete this session? Its running terminals will be closed.",
                );
                "Delete session"
            }
            Dialog::CloseTab(_) => {
                content = content.child("Close this terminal and its running processes?");
                "Close terminal"
            }
            Dialog::Quit => {
                let question = self
                    .close_confirmation
                    .as_ref()
                    .map(|confirmation| {
                        format!(
                            "Close {} shell{} and quit?",
                            confirmation.terminal_count,
                            if confirmation.terminal_count == 1 {
                                ""
                            } else {
                                "s"
                            }
                        )
                    })
                    .unwrap_or_else(|| "Close shells and quit?".into());
                content=content.child(question).child(div().text_color(palette.muted).child("Running commands will stop. Shells restart fresh next time. External terminals stay open."));
                "Quit Shellfox?"
            }
            Dialog::Create(_) => return div().into_any_element(),
        };
        let cancel = button("dialog-cancel", "Cancel", palette)
            .on_click(cx.listener(|root, _, window, cx| root.dismiss_dialog(window, cx)));
        let confirm_label = match dialog {
            Dialog::Quit => "Close shells and quit",
            Dialog::CloseTab(_) => "Close",
            Dialog::DeleteSession(_) => "Delete",
            _ => "Save",
        };
        let confirm = button("dialog-confirm", confirm_label, palette)
            .on_click(cx.listener(|root, _, window, cx| root.confirm_dialog(window, cx)));
        let actions = div().flex().justify_end().gap(px(6.));
        content = content.child(if matches!(dialog, Dialog::Quit) {
            actions.child(cancel).child(confirm)
        } else {
            actions.child(confirm).child(cancel)
        });
        self.modal_frame(title, content.into_any_element(), palette, cx)
    }
    fn confirm_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.user_action_pending {
            return;
        }
        let Some(dialog) = &self.dialog else {
            return;
        };
        let action = match dialog {
            Dialog::RenameSession { id, title } => {
                NativeAction::Metadata(HostRequest::RenameSession {
                    session_id: *id,
                    title: title.read(cx).text.clone(),
                })
            }
            Dialog::RenameTab { id, title } => {
                let Some(session) = self.snapshot.as_ref().and_then(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .find(|session| session.tabs.iter().any(|tab| tab.id == *id))
                }) else {
                    return;
                };
                NativeAction::Metadata(HostRequest::RenameTab {
                    session_id: session.id,
                    tab_id: *id,
                    title: title.read(cx).text.clone(),
                })
            }
            Dialog::CloseTab(id) => NativeAction::CloseTab { tab: *id },
            Dialog::DeleteSession(id) => NativeAction::DeleteSession { session: *id },
            Dialog::Quit => {
                self.begin_shutdown(window, cx);
                return;
            }
            Dialog::Create(_) => return,
        };
        self.submit_native(action, AfterAction::Dialog, window, cx);
    }
}
