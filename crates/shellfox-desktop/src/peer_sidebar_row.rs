use super::*;
use crate::root::peer::sidebar_layout::RowFocus;
use crate::{assets::inline_ellipsis, root::peer::session_dialog::Editor, session_menu::Item};
use gpui::{Bounds, KeyDownEvent, Pixels};
use std::{cell::Cell, rc::Rc};

impl Root {
    pub(in crate::root::peer) fn peer_sidebar_row(
        &self,
        cycle: &source::PeerCycle,
        session: &m::Session,
        dot: AgentState,
        selected: bool,
        focus: RowFocus,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = session.id;
        let menu_focus = focus.0.clone();
        let target = cycle.clone();
        let mouse_menu = cycle.clone();
        let key_menu = cycle.clone();
        let toggle = cycle.clone();
        let rename = cycle.clone();
        let settled = dot == AgentState::Settled;
        let cwd = session.facts.cwd.known().map(String::as_str).unwrap_or("");
        let pending = self.peer_ui.pending.contains(&cycle.peer);
        let background = palette.accent.opacity(0.09);
        let bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Default::default();
        let measured = bounds.clone();
        let editing = match &self.dialog {
            Some(Dialog::PeerSession(dialog)) if dialog.cycle == *cycle && dialog.id == id => {
                if let Editor::Rename(field) = &dialog.editor {
                    Some(field.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        let mut copy = div()
            .id("select-peer-session")
            .relative()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .line_height(px(14.));
        if let Some(field) = &editing {
            copy = copy.child(field.clone());
        } else {
            copy = copy
                .track_focus(&focus.0)
                .when(
                    self.sidebar_ui.keyboard_focus && focus.0.is_focused(window),
                    |copy| copy.child(crate::sidebar::row::focus_ring(palette)),
                )
                .cursor_pointer()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .gap(px(4.))
                        .items_center()
                        .child(
                            div()
                                .id("peer-session-title")
                                .min_w_0()
                                .text_size(px(13.))
                                .when(settled, |style| {
                                    style.text_color(palette.status_color(0x96734e))
                                })
                                .tooltip(tooltip(session.title.clone(), palette))
                                .child(inline_ellipsis(session.title.clone(), 14.))
                                .on_click(cx.listener(
                                    move |root, event: &gpui::ClickEvent, window, cx| {
                                        if event.click_count() > 1 {
                                            cx.stop_propagation();
                                            root.open_peer_session_dialog(
                                                rename.clone(),
                                                id,
                                                Item::Rename,
                                                window,
                                                cx,
                                            );
                                        }
                                    },
                                )),
                        )
                        .when(
                            session.facts.pinned_at.known().is_some_and(Option::is_some),
                            |line| {
                                line.child(
                                    div()
                                        .id("peer-session-pin")
                                        .flex_shrink_0()
                                        .text_color(palette.muted)
                                        .tooltip(tooltip("Pinned", palette))
                                        .child(icon_sized("pin", 11.)),
                                )
                            },
                        ),
                )
                .on_click(
                    cx.listener(move |root, event: &gpui::ClickEvent, window, cx| {
                        if event.click_count() == 1 {
                            root.activate_peer_session(target.clone(), id, window, cx);
                        }
                    }),
                );
        }
        copy = copy.child(
            div()
                .id("peer-session-path")
                .w_full()
                .min_w_0()
                .text_size(px(11.))
                .text_color(palette.muted)
                .tooltip(tooltip(cwd.to_owned(), palette))
                .child(ellipsis(shorten_home(cwd), 14.)),
        );
        div()
            .id(gpui::SharedString::from(format!(
                "peer-{}-session-{id}",
                cycle.peer.as_str()
            )))
            .relative()
            .h(px(36.))
            .flex()
            .items_start()
            .gap(px(8.))
            .px(px(8.))
            .py(px(4.))
            .border_l_2()
            .border_color(if selected {
                palette.accent
            } else {
                gpui::transparent_black()
            })
            .when(selected, |style| style.bg(background))
            .hover(move |style| {
                style.bg(if selected {
                    background
                } else {
                    palette.row_hover
                })
            })
            .child(
                gpui::canvas(|_, _, _| (), move |b, _, _, _| measured.set(Some(b)))
                    .absolute()
                    .inset_0(),
            )
            .child(self.semantic_region(
                &format!("sidebar/{}/{id}/status", cycle.peer.as_str()),
                crate::accessibility::Role::Image,
                status_label(dot, error_text(session, false)),
                status_dot(dot, palette).mt(px(3.)),
            ))
            .children(id.to_string().parse().ok().map(|id| {
                self.sidebar_ui.visibility.marker(crate::session_visibility::PollingTarget::Peer(cycle.peer.clone(), id))
            }))
            .child(copy)
            .child(
                icon_button(
                    "peer-session-settle",
                    if settled { "unsettle" } else { "settle" },
                    palette,
                )
                .w(px(20.))
                .h(px(24.))
                .tooltip(tooltip(
                    if settled { "Unsettle" } else { "Settle" },
                    palette,
                ))
                .when(pending, |button| button.opacity(0.4))
                .when(!pending, |button| {
                    button.track_focus(&focus.1).on_click(cx.listener(
                        move |root, _, window, cx| {
                            root.toggle_peer_settlement(toggle.clone(), id, settled, window, cx)
                        },
                    ))
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |root, event: &gpui::MouseDownEvent, window, cx| {
                    root.sidebar_ui.keyboard_focus = false;
                    if !matches!(&root.dialog, Some(Dialog::PeerSession(dialog)) if dialog.cycle == mouse_menu && dialog.id == id && matches!(&dialog.editor, Editor::Rename(_))) { menu_focus.focus(window); }
                    root.open_peer_session_menu(
                        mouse_menu.clone(),
                        id,
                        event.position,
                        false,
                        window,
                        cx,
                    );
                }),
            )
            .on_key_down(cx.listener(move |root, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key == "f10" && event.keystroke.modifiers.shift
                    || matches!(key, "contextmenu" | "context-menu" | "menu")
                {
                    if let Some(bounds) = bounds.get() {
                        root.open_peer_session_menu(
                            key_menu.clone(),
                            id,
                            gpui::point(bounds.left(), bounds.bottom()),
                            true,
                            window,
                            cx,
                        );
                    }
                } else if root.peer_row_navigation(&key_menu, id, event, window, cx) {
                    cx.notify();
                } else if let Some(field) = &editing
                    && field.read(cx).edit_focus().is_focused(window)
                {
                    if field.read(cx).is_composing() {
                        return;
                    }
                    match key {
                        "enter" => root.confirm_peer_session_dialog(window, cx),
                        "escape" if !root.dialog_locked() => {
                            root.dismiss_peer_session_dialog(window, cx)
                        }
                        _ => return,
                    }
                } else {
                    return;
                }
                window.prevent_default();
                cx.stop_propagation();
            }))
            .into_any_element()
    }
}
