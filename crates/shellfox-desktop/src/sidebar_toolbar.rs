use super::*;
use crate::accessibility::{Action, Role, SemanticAction, SemanticActionKind, SemanticNode};
use gpui::{AnyElement, Stateful, Window};

const SETTINGS_BUTTON: &str = "chrome/sidebar/settings";

impl Root {
    fn toggle_settings_from_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings && self.pending_access_count() > 0 {
            self.review_access_requests(window, cx);
        } else {
            self.open_settings(cx);
        }
    }

    fn settings_button_semantics(&self, button: Stateful<gpui::Div>, hint: &str) -> AnyElement {
        let frame = self.semantic_context.frame();
        match frame.id_for(SETTINGS_BUTTON) {
            Ok(id) => {
                let mut node =
                    SemanticNode::new(id, Some(frame.root_id()), Role::Button, "Settings");
                if self.pending_access_count() != 0 {
                    node.data.set_description(hint);
                }
                node.data.set_toggled(if self.settings {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                });
                if self.snapshot.is_some() && !self.closing_window {
                    node.data.add_action(Action::Click);
                    node.focus = Some(self.sidebar_ui.settings.clone());
                } else {
                    node.data.set_disabled();
                }
                frame.wrap(node, button).into_any_element()
            }
            Err(error) => {
                frame.reject(error);
                button.into_any_element()
            }
        }
    }

    pub(crate) fn handle_sidebar_settings_action(
        &mut self,
        action: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let frame = self.semantic_context.frame();
        if !matches!(action.kind, SemanticActionKind::Click)
            || self.dialog.is_some()
            || self.tab_menu.is_some()
            || self.profile_menu.is_some()
            || frame.id_for(SETTINGS_BUTTON).ok() != Some(action.target)
            || frame.focus_handle(action.target).is_none()
        {
            return false;
        }
        self.toggle_settings_from_toolbar(window, cx);
        true
    }

    pub(super) fn sidebar_toolbar(&self, palette: Palette, cx: &Context<Self>) -> gpui::Div {
        let create_ready = !self.user_action_pending
            && (self.can_create_local_session()
                || self.peers.peers.values().any(|peer| peer.remembered));
        let new_session_anchor = self.sidebar_ui.new_session_anchor.clone();
        let requests = self.pending_access_count();
        let settings_hint = if requests == 0 {
            "Settings".to_owned()
        } else {
            let suffix = if requests == 1 { "" } else { "s" };
            format!("Settings · {requests} access request{suffix}")
        };
        div()
            .h(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px(px(2.))
            .gap(px(4.))
            .border_b_1()
            .border_color(palette.border)
            .child(
                icon_button("hide-sessions", "back", palette)
                    .w(px(24.))
                    .track_focus(&self.sidebar_ui.toggle)
                    .focus(|style| style.border_1().border_color(palette.accent))
                    .tooltip(tooltip(crate::layout::sessions_tooltip(true), palette))
                    .on_click(cx.listener(|root, _, window, cx| {
                        root.sidebar_ui.toggle.focus(window);
                        root.toggle_sessions_layout(window, cx);
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        toolbar_tip(
                            icon_button("new-session", "plus", palette)
                                .when(self.sidebar_ui.target_menu.is_some(), |button| {
                                    button.text_color(palette.accent)
                                })
                                .relative()
                                .child(
                                    gpui::canvas(
                                        |_, _, _| (),
                                        move |bounds, _, _, _| new_session_anchor.set(Some(bounds)),
                                    )
                                    .absolute()
                                    .inset_0(),
                                ),
                            if self.user_action_pending {
                                "Preparing session…"
                            } else {
                                "New session"
                            },
                            palette,
                        )
                        .when(!create_ready, |style| style.opacity(0.4))
                        .when(create_ready, |button| {
                            button
                                .track_focus(&self.sidebar_ui.new_session)
                                .focus(|style| style.border_1().border_color(palette.accent))
                                .on_click(cx.listener(
                                    |root, event: &gpui::ClickEvent, window, cx| {
                                        root.new_session_from_toolbar(
                                            matches!(event, gpui::ClickEvent::Keyboard(_)),
                                            window,
                                            cx,
                                        )
                                    },
                                ))
                        }),
                    )
                    .child(self.render_power_button(palette, cx))
                    .child(
                        self.settings_button_semantics(
                            toolbar_tip(
                                icon_button("settings", "settings", palette)
                                    .when(requests > 0, |button| {
                                        button.child(
                                            div()
                                                .absolute()
                                                .right(px(1.))
                                                .top(px(1.))
                                                .min_w(px(13.))
                                                .h(px(13.))
                                                .px(px(2.))
                                                .rounded(px(6.))
                                                .bg(palette.accent)
                                                .text_color(palette.on_accent())
                                                .text_size(px(9.))
                                                .line_height(px(9.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(if requests > 9 {
                                                    "9+".into()
                                                } else {
                                                    requests.to_string()
                                                }),
                                        )
                                    })
                                    .track_focus(&self.sidebar_ui.settings)
                                    .on_key_down(cx.listener(
                                        |root, event: &gpui::KeyDownEvent, window, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                root.toggle_settings_from_toolbar(window, cx);
                                                window.prevent_default();
                                                cx.stop_propagation();
                                            }
                                        },
                                    )),
                                settings_hint.clone(),
                                palette,
                            )
                            .when(self.settings, |style| style.text_color(palette.accent))
                            .on_click(cx.listener(
                                |root, _, window, cx| root.toggle_settings_from_toolbar(window, cx),
                            )),
                            &settings_hint,
                        ),
                    ),
            )
    }
}
