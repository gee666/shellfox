use super::*;
use crate::sidebar::RowFocus;
use crate::{accessibility::{SemanticAction, SemanticActionKind}, assets::inline_ellipsis,
    root::peer::session_dialog::Editor, session_menu::Item};
use gpui::{Bounds, KeyDownEvent, Pixels};
use std::{cell::Cell, rc::Rc};

fn workspace_session_key(peer: &shellfox_protocol::AuthenticatedPeerId, session: m::Id) -> String {
    format!("workspace/peer/{}/session/{session}", peer.as_str())
}

impl Root {
    pub(crate) fn handle_workspace_navigation(
        &mut self, action: &SemanticAction, window: &mut Window, cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(action.kind, SemanticActionKind::Click) || self.settings || self.dialog.is_some() || self.closing_window {
            return false;
        }
        let frame = self.semantic_context.frame();
        let matches = |key: &str| frame.id_for(key).ok() == Some(action.target);
        if matches("workspace/peer/terminal-folder") && !self.is_local_source() {
            self.open_peer_terminal_folder(cx);
            return true;
        }
        if matches("workspace/local/sessions") && self.sidebar && self.snapshot.is_some() {
            self.toggle_local_group(window, cx);
            return true;
        }
        let target = self.peers.peers.iter().find_map(|(peer, state)| {
            if !state.remembered || state.phase != source::Phase::Connected || self.peer_ui.collapsed.contains(peer) {
                return None;
            }
            let session = source::sessions(state.snapshot.as_ref()?)
                .find(|session| matches(&workspace_session_key(peer, session.id)))?;
            Some((source::PeerCycle { peer: peer.clone(), generation: state.cycle }, session.id))
        });
        if let Some((cycle, id)) = target {
            self.activate_peer_session(cycle, id, window, cx);
            return true;
        }
        false
    }

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
        let menu_focus = focus.copy.clone();
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
                .track_focus(&focus.copy)
                .when(
                    self.sidebar_ui.keyboard_focus && focus.copy.is_focused(window),
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
            .track_focus(&focus.row)
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
            .child(if editing.is_some() { copy.into_any_element() } else {
                self.workspace_button(&workspace_session_key(&cycle.peer, id), format!("Select session {}", session.title), copy)
            })
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
                    button.track_focus(&focus.settle).on_click(cx.listener(
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
