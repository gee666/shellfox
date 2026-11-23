pub(crate) mod directory;
pub(crate) mod environment;
mod state;
use crate::{
    assets::icon_button, field::Field, root::Root, theme::Palette, workspace::AfterAction,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use shellfox_host::{HostRequest, SessionId, TabId};
use shellfox_service::native::NativeAction;
pub(crate) use state::{Dialog, DialogState};

/// The request Enter and blur commit for a tab title being edited in place.
fn tab_rename(
    session_id: SessionId,
    tab_id: TabId,
    text: &str,
) -> Result<HostRequest, &'static str> {
    shellfox_core::validation::title(text).map_err(
        |_| "Use a nonblank title of at most 200 characters, without control characters.",
    )?;
    Ok(HostRequest::RenameTab {
        session_id,
        tab_id,
        title: text.trim().to_owned(),
    })
}

pub(crate) fn dialog_button(
    id: &'static str,
    label: &'static str,
    palette: Palette,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(8.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_size(px(12.))
        .line_height(px(16.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface)
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover))
        .child(label)
}

impl Root {
    pub(crate) fn rename_session_dialog(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialogs.user_action_pending || self.close.closing {
            return;
        }
        if let Some(session) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
        {
            let title = Field::session_title(session.title.clone(), cx);
            title.read(cx).focus(window);
            self.dialogs.current = Some(Dialog::RenameSession { id, title });
        }
        cx.notify();
    }
    pub(crate) fn tab_title_editor(
        &self,
        title: gpui::Entity<crate::field::Field>,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id("rename-terminal")
            .w(px(130.))
            .mx(px(4.))
            .mt(px(2.))
            .child(title.clone())
            .on_mouse_down_out(cx.listener(|root, _, window, cx| {
                if matches!(root.dialogs.current, Some(Dialog::RenameTab { .. })) {
                    root.confirm_dialog(window, cx);
                }
            }))
            .on_key_down(
                cx.listener(move |root, event: &gpui::KeyDownEvent, window, cx| {
                    if title.read(cx).is_composing() {
                        return;
                    }
                    match event.keystroke.key.as_str() {
                        "escape" => root.dismiss_dialog(window, cx),
                        "enter" => root.confirm_dialog(window, cx),
                        "tab" => root.move_strip_focus(event.keystroke.modifiers.shift, window, cx),
                        _ => return,
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    pub(crate) fn rename_tab_dialog(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialogs.user_action_pending || self.close.closing {
            return;
        }
        if let Some(tab) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .sessions
                .iter()
                .flat_map(|session| &session.tabs)
                .find(|tab| tab.id == id)
        }) {
            let title = Field::tab_title(self.display_tab_title(tab).to_owned(), cx);
            let field = title.entity_id();
            let blur = cx.on_blur(&title.read(cx).edit_focus(), window, move |root, window, cx| {
                if matches!(&root.dialogs.current, Some(Dialog::RenameTab { title, .. }) if title.entity_id() == field) {
                    root.confirm_dialog(window, cx);
                }
            });
            title.read(cx).focus(window);
            self.dialogs.current = Some(Dialog::RenameTab {
                id,
                title,
                _blur: blur,
            });
        }
        cx.notify();
    }
    pub(crate) fn modal_frame(
        &self,
        title: impl Into<gpui::SharedString>,
        content: AnyElement,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let black: gpui::Hsla = gpui::rgb(0).into();
        let title = title.into();
        let quitting = matches!(self.dialogs.current, Some(Dialog::Quit));
        let (width, radius) = match &self.dialogs.current {
            _ if self.peer_environment_is_open() => (480., 8.),
            Some(Dialog::Quit) => (360., 4.),
            Some(Dialog::SessionEnvironment { .. }) => (480., 8.),
            _ => (540., 4.),
        };
        let close = icon_button("close-dialog", "close", palette)
            .when_some(
                match &self.dialogs.current {
                    Some(Dialog::Create(draft)) => Some(&draft.close),
                    Some(Dialog::Quit) => Some(&self.close.quit_focus.header),
                    _ => None,
                },
                |button, focus| {
                    button
                        .track_focus(focus)
                        .focus(|style| style.border_1().border_color(palette.accent))
                },
            )
            .on_click(cx.listener(|root, _, window, cx| root.dismiss_active_modal(window, cx)));
        let close = if quitting {
            self.quit_close_button(close.when(self.dialog_locked(), |button| button.opacity(0.45)))
        } else {
            close.into_any_element()
        };
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
                cx.listener(|root, _, window, cx| root.dismiss_active_modal(window, cx)),
            )
            .child(
                self.semantic_region(
                    "root/dialog",
                    crate::accessibility::Role::Dialog,
                    title.to_string(),
                    div()
                        .when(!quitting || self.dialog_locked(), |card| {
                            card.track_focus(&self.dialogs.modal_focus)
                        })
                        .capture_key_up(cx.listener(Self::directory_key_up))
                        .on_key_down(cx.listener(|root, event: &gpui::KeyDownEvent, window, cx| {
                            if matches!(root.dialogs.current, Some(Dialog::Create(_))) {
                                root.directory_key(event, window, cx);
                                return;
                            }
                            if root.dialog_locked() {
                                if matches!(root.dialogs.current, Some(Dialog::Quit)) {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                }
                                return;
                            }
                            if matches!(root.dialogs.current, Some(Dialog::Quit)) {
                                root.quit_dialog_key(event, window, cx);
                                return;
                            }
                            match event.keystroke.key.as_str() {
                                "escape" => root.dismiss_dialog(window, cx),
                                "enter"
                                    if matches!(
                                        root.dialogs.current,
                                        Some(Dialog::SessionEnvironment { .. })
                                    ) || root.peer_environment_is_open() =>
                                {
                                    if !event.keystroke.modifiers.control {
                                        return;
                                    }
                                    root.confirm_dialog(window, cx);
                                }
                                "enter" => root.confirm_dialog(window, cx),
                                _ => return,
                            }
                            window.prevent_default();
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .w(px(width))
                        .max_w_full()
                        .flex()
                        .flex_col()
                        .rounded(px(radius))
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
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .when(
                                            matches!(
                                                self.dialogs.current,
                                                Some(
                                                    Dialog::Create(_)
                                                        | Dialog::SessionEnvironment { .. }
                                                )
                                            ) || self.peer_environment_is_open(),
                                            |label| label.relative().top(px(2.)),
                                        )
                                        .child(title),
                                )
                                .child(close),
                        )
                        .child(content),
                ),
            );
        self.semantics
            .context
            .frame()
            .overlay_subtree(dialog)
            .into_any_element()
    }
    fn dismiss_active_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.dialogs.current, Some(Dialog::Create(_))) {
            self.dismiss_directory(window, cx);
        } else {
            self.dismiss_dialog(window, cx);
        }
    }

    pub(crate) fn render_dialog(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        match &self.dialogs.current {
            Some(Dialog::Create(draft)) => self.render_directory(draft, palette, window, cx),
            Some(Dialog::Quit) => self.render_quit_dialog(palette, window, cx),
            Some(Dialog::SessionEnvironment { id, text }) => {
                self.session_environment_dialog(*id, text, palette, window, cx)
            }
            Some(Dialog::PeerSession(dialog)) => {
                self.render_peer_session_environment(dialog, palette, window, cx)
            }
            _ => div().into_any_element(),
        }
    }

    pub(crate) fn confirm_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialogs.user_action_pending {
            return;
        }
        let Some(dialog) = &self.dialogs.current else {
            return;
        };
        let sidebar = matches!(
            dialog,
            Dialog::RenameSession { .. }
                | Dialog::SessionEnvironment { .. }
                | Dialog::DeleteSession(_)
        );
        let action = match dialog {
            Dialog::RenameSession { id, title } => {
                let title = title.read(cx).text.trim();
                if title.is_empty() {
                    return;
                }
                NativeAction::Metadata(HostRequest::RenameSession {
                    session_id: *id,
                    title: title.to_owned(),
                })
            }
            Dialog::RenameTab { id, title, .. } => {
                if title.read(cx).is_composing() {
                    return;
                }
                let Some(session) = self.snapshot.as_ref().and_then(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .find(|session| session.tabs.iter().any(|tab| tab.id == *id))
                }) else {
                    return;
                };
                match tab_rename(session.id, *id, &title.read(cx).text) {
                    Ok(request) => NativeAction::Metadata(request),
                    Err(message) => {
                        self.notify_error(None, Some("VALIDATION".into()), message, cx);
                        return;
                    }
                }
            }
            Dialog::SessionEnvironment { id, text } => {
                let (env, issues) =
                    crate::settings::environment_data::parse(&text.read(cx).text, cfg!(windows));
                if !issues.is_empty() {
                    return;
                }
                NativeAction::Metadata(HostRequest::SetSessionEnv {
                    session_id: *id,
                    env,
                })
            }
            Dialog::PeerSession(_) => {
                self.confirm_peer_session_dialog(window, cx);
                return;
            }
            Dialog::CloseTab(target) => NativeAction::CloseTerminal(*target),
            Dialog::DeleteSession(id) => NativeAction::DeleteSession { session: *id },
            Dialog::Quit => {
                self.begin_shutdown(window, cx);
                return;
            }
            Dialog::Create(_) => return,
        };
        if sidebar {
            self.submit_sidebar_action(action, AfterAction::Dialog, window, cx);
        } else {
            self.submit_native(action, AfterAction::Dialog, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committing_a_tab_title_sends_the_trimmed_name_and_rejects_blank_or_control_text() {
        let (session, tab) = (SessionId::new(), TabId::new());
        match tab_rename(session, tab, "  build logs ") {
            Ok(HostRequest::RenameTab {
                session_id,
                tab_id,
                title,
            }) => {
                assert_eq!(
                    (session_id, tab_id, title.as_str()),
                    (session, tab, "build logs")
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        for bad in ["", "   ", "a\u{7}b", &"x".repeat(201)] {
            assert!(tab_rename(session, tab, bad).is_err(), "{bad:?}");
        }
    }
}
