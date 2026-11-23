use crate::{
    root::{Dialog, Root},
    theme::Palette,
    workspace::AfterAction,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use shellfox_core::TerminalIdentity;
use shellfox_service::native::NativeAction;

pub(super) struct CloseButton {
    pub(super) target: TerminalIdentity,
    pub(super) visible: bool,
    pub(super) locked: bool,
}

impl Root {
    pub(super) fn tab_close_button(
        &self,
        state: CloseButton,
        title: &str,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let key = super::scroll::TabFocus::Close(state.target);
        let button = crate::assets::icon_button_sized(
            format!("close-{}", state.target.tab_id), "close", 24., 12., palette,
        )
        .mt(px(2.))
        .mr(px(2.))
        .when(!state.visible, |button| {
            button.opacity(0.).group_hover("terminal-tab", |style| style.opacity(1.))
        })
        .when(state.locked, |button| button.cursor(gpui::CursorStyle::Arrow));
        let focus = (!state.locked).then(|| self.tab_scroll.control_focus(key, cx).tab_stop(true));
        self.tab_strip_control(key, format!("Close terminal {title}"), button, focus, palette, cx)
    }

    pub(crate) fn request_tab_close(
        &mut self,
        target: TerminalIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window
            || self.user_action_pending
            || self.dialog.is_some()
        {
            return;
        }
        if self.tab_close_identity(target.tab_id) != Some(target) {
            self.error = Some("Terminal generation is no longer current.".into());
            cx.notify();
            return;
        }
        if self.wsl_tab_has_draft(target.tab_id, cx) {
            self.error = Some("Save or close this tab's Files draft before closing the terminal.".into());
            cx.notify();
            return;
        }
        if self.tab_has_agents(target.tab_id) {
            self.dialog = Some(Dialog::CloseTab(target));
            self.modal_focus.focus(window);
            cx.notify();
        } else {
            self.submit_native(
                NativeAction::CloseTerminal(target),
                AfterAction::Mutation,
                window,
                cx,
            );
        }
    }

    pub(crate) fn render_tab_close_confirmation(
        &self,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(Dialog::CloseTab(target)) = self.dialog else {
            return div().into_any_element();
        };
        let title = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .sessions
                    .iter()
                    .flat_map(|session| &session.tabs)
                    .find(|tab| tab.id == target.tab_id)
            })
            .map_or_else(
                || "Close terminal?".to_owned(),
                |tab| format!("Close terminal {}?", self.display_tab_title(tab)),
            );
        let pending = self.user_action_pending;
        let card = div()
            .id("confirm-close-terminal")
            .absolute()
            .top(px(36.))
            .left(px(8.))
            .max_w_full()
            .occlude()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.))
            .line_height(px(16.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .rounded(px(6.))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.4),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
            }])
            .on_key_down(cx.listener(|root, event: &gpui::KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => root.dismiss_dialog(window, cx),
                    "tab" if event.keystroke.modifiers.shift => window.focus_prev(),
                    "tab" => window.focus_next(),
                    _ => return,
                }
                window.prevent_default();
                cx.stop_propagation();
            }))
            .child("An agent is running in this tab. Close?")
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(4.))
                    .child(
                        crate::dialogs::dialog_button("close-terminal", "Close", palette)
                            .track_focus(&self.modal_focus)
                            .tab_index(0)
                            .focus(|style| style.border_color(palette.accent))
                            .when(pending, |button| button.opacity(0.4))
                            .when(!pending, |button| {
                                button
                                    .on_click(cx.listener(|root, _, window, cx| {
                                        root.confirm_dialog(window, cx);
                                    }))
                                    .on_key_down(cx.listener(
                                        |root, event: &gpui::KeyDownEvent, window, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                root.confirm_dialog(window, cx);
                                                cx.stop_propagation();
                                            }
                                        },
                                    ))
                            }),
                    )
                    .child(
                        crate::dialogs::dialog_button("cancel-close-terminal", "Cancel", palette)
                            .focusable()
                            .tab_index(0)
                            .focus(|style| style.border_color(palette.accent))
                            .on_click(
                                cx.listener(|root, _, window, cx| root.dismiss_dialog(window, cx)),
                            )
                            .on_key_down(cx.listener(
                                |root, event: &gpui::KeyDownEvent, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        root.dismiss_dialog(window, cx);
                                        window.prevent_default();
                                        cx.stop_propagation();
                                    }
                                },
                            )),
                    ),
            );
        let frame = self.semantic_context.frame();
        match frame.id_for("workspace/close-terminal") {
            Ok(node) => frame
                .wrap(
                    crate::accessibility::SemanticNode::new(
                        node,
                        Some(frame.root_id()),
                        crate::accessibility::Role::Dialog,
                        title,
                    ),
                    card,
                )
                .into_any_element(),
            Err(error) => {
                frame.reject(error);
                card.into_any_element()
            }
        }
    }
}
