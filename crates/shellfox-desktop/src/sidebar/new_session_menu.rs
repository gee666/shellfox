use super::*;
use crate::peers::source::{Phase, Source};
use gpui::{Bounds, KeyDownEvent, KeyUpEvent, Pixels, Point, Window};
use shellfox_protocol::AuthenticatedPeerId;

pub(super) struct NewSessionMenu {
    position: Point<Pixels>,
    active: Option<usize>,
    show_focus: bool,
}
struct Choice {
    id: Option<AuthenticatedPeerId>,
    name: String,
    state: Option<&'static str>,
    enabled: bool,
    selected: bool,
    color: gpui::Hsla,
}

fn move_target_focus(enabled: &[usize], current: Option<usize>, key: &str) -> Option<usize> {
    let first = *enabled.first()?;
    let index = current.and_then(|current| enabled.iter().position(|item| *item == current));
    match (key, index) {
        ("up", Some(index)) => Some(enabled[(index + enabled.len() - 1) % enabled.len()]),
        ("down", Some(index)) => Some(enabled[(index + 1) % enabled.len()]),
        ("up" | "end", _) => enabled.last().copied(),
        _ => Some(first),
    }
}

impl Root {
    fn session_targets(&self, palette: Palette) -> Vec<Choice> {
        let mut peers: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter(|(_, state)| state.remembered)
            .collect();
        peers.sort_by(|a, b| a.1.hostname.cmp(&b.1.hostname));
        let local = Choice {
            id: None,
            name: "Local".into(),
            state: None,
            enabled: self.can_create_local_session(),
            selected: self.is_local_source(),
            color: palette.muted,
        };
        std::iter::once(local)
            .chain(peers.into_iter().map(|(id, peer)| {
                let connected = peer.phase == Phase::Connected;
                let state = if connected {
                    "Connected"
                } else if peer.online {
                    "Connect to continue"
                } else {
                    "Offline"
                };
                Choice {
                    id: Some(id.clone()),
                    name: peer.hostname.clone(),
                    state: Some(state),
                    enabled: connected || peer.online,
                    selected: self.peers.selected == Source::Peer(id.clone()),
                    color: crate::peers::sidebar::connection_color(peer, palette),
                }
            }))
            .collect()
    }

    pub(super) fn new_session_from_toolbar(
        &mut self,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.user_action_pending || self.closing_window || self.dialog.is_some() {
            return;
        }
        if !self.peers.peers.values().any(|peer| peer.remembered) {
            self.open_local_directory(window, cx);
            return;
        }
        if self.sidebar_ui.target_menu.is_some() {
            self.close_new_session_menu(window, cx);
            return;
        }
        let choices = self.session_targets(Palette::new("#ec4899", "#111016"));
        let active = choices
            .iter()
            .position(|item| item.enabled && item.selected)
            .or_else(|| choices.iter().position(|item| item.enabled));
        let position = self.sidebar_ui.new_session_anchor.get().map_or_else(
            || window.mouse_position(),
            |bounds: Bounds<Pixels>| gpui::point(bounds.left(), bounds.bottom() + px(5.)),
        );
        self.session_menu = None;
        self.sidebar_ui.target_menu = Some(NewSessionMenu {
            position,
            active,
            show_focus: keyboard,
        });
        self.modal_focus.focus(window);
        cx.notify();
    }

    pub(crate) fn render_new_session_menu(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.sidebar_ui.target_menu.as_ref()?;
        let choices = self.session_targets(palette);
        let viewport = window.viewport_size();
        let width = 234_f32.min((f32::from(viewport.width) - 14.).max(144.));
        let items_height = (38. + choices.len().saturating_sub(1) as f32 * 47.).min(280.);
        let height = (37. + items_height).min(f32::from(viewport.height) - 8.);
        let x = menu
            .position
            .x
            .clamp(px(4.), (viewport.width - px(width + 4.)).max(px(4.)));
        let y = menu
            .position
            .y
            .clamp(px(4.), (viewport.height - px(height + 4.)).max(px(4.)));
        let mut controls = div()
            .id("new-session-targets")
            .max_h(px((height - 37.).max(0.)))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for (index, choice) in choices.into_iter().enumerate() {
            let id = choice.id;
            controls = controls.child(
                div()
                    .id(("new-session-target", index))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .h(px(if id.is_some() { 47. } else { 38. }))
                    .flex_shrink_0()
                    .px(px(8.))
                    .py(px(7.))
                    .rounded(px(3.))
                    .child(
                        icon_sized(if id.is_some() { "server" } else { "monitor" }, 15.)
                            .text_color(palette.muted)
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .child(div().truncate().child(choice.name))
                            .when_some(choice.state, |style, detail| {
                                style.child(
                                    div()
                                        .text_size(px(10.))
                                        .line_height(px(14.))
                                        .mt(px(3.))
                                        .text_color(palette.muted)
                                        .child(detail),
                                )
                            }),
                    )
                    .when(choice.selected, |style| {
                        style.child(icon_sized("check", 15.).text_color(palette.accent))
                    })
                    .when(!choice.selected && id.is_some(), |style| {
                        style.child(div().size(px(5.)).rounded_full().bg(choice.color))
                    })
                    .when(menu.show_focus && menu.active == Some(index), |style| {
                        style
                            .border_1()
                            .border_color(palette.accent)
                            .bg(palette.hover)
                    })
                    .when(!choice.enabled, |style| style.opacity(0.4))
                    .when(choice.enabled, |style| {
                        style
                            .cursor_pointer()
                            .hover(|style| style.bg(palette.hover))
                            .on_click(cx.listener(move |root, _, window, cx| {
                                root.select_session_target(id.clone(), window, cx)
                            }))
                    }),
            );
        }
        Some(
            div()
                .id("new-session-target-menu")
                .track_focus(&self.modal_focus)
                .absolute()
                .occlude()
                .left(x)
                .top(y)
                .w(px(width))
                .p(px(4.))
                .rounded(px(6.))
                .border_1()
                .border_color(palette.border)
                .bg(palette.panel)
                .shadow_lg()
                .child(
                    div()
                        .text_size(px(10.))
                        .line_height(px(14.))
                        .text_color(palette.muted)
                        .px(px(8.))
                        .pt(px(6.))
                        .pb(px(7.))
                        .child("New session on"),
                )
                .child(controls)
                .on_mouse_down_out(
                    cx.listener(|root, _, window, cx| root.close_new_session_menu(window, cx)),
                )
                .on_key_down(cx.listener(move |root, e: &KeyDownEvent, window, cx| {
                    root.new_session_menu_key(e, palette, window, cx)
                }))
                .on_key_up(cx.listener(move |root, e: &KeyUpEvent, window, cx| {
                    if !matches!(e.keystroke.key.as_str(), "enter" | "space") {
                        return;
                    }
                    let Some(menu) = &root.sidebar_ui.target_menu else {
                        return;
                    };
                    let choices = root.session_targets(palette);
                    if let Some(choice) = menu.active.and_then(|index| choices.get(index)).filter(|choice| choice.enabled) {
                        root.select_session_target(choice.id.clone(), window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }))
                .into_any_element(),
        )
    }

    fn new_session_menu_key(
        &mut self,
        event: &KeyDownEvent,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let choices = self.session_targets(palette);
        let Some(menu) = &mut self.sidebar_ui.target_menu else {
            return;
        };
        let enabled: Vec<_> = choices
            .iter()
            .enumerate()
            .filter(|(_, item)| item.enabled)
            .map(|(index, _)| index)
            .collect();
        match event.keystroke.key.as_str() {
            "escape" | "tab" => self.close_new_session_menu(window, cx),
            "enter" | "space" => {}
            key @ ("up" | "down" | "home" | "end") => {
                menu.show_focus = true;
                menu.active = move_target_focus(&enabled, menu.active, key);
            }
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    fn close_new_session_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_ui.target_menu.take().is_some() {
            self.sidebar_ui.new_session.focus(window);
            cx.notify();
        }
    }

    fn select_session_target(
        &mut self,
        target: Option<AuthenticatedPeerId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_new_session_menu(window, cx);
        if let Some(peer) = target {
            let ready =
                self.peers.peers.get(&peer).is_some_and(|state| {
                    state.phase == Phase::Connected && state.snapshot.is_some()
                });
            self.peer_ui.pending_create = (!ready).then(|| peer.clone());
            self.choose_peer(peer, window, cx);
            if ready {
                self.open_peer_directory(window, cx);
            }
        } else {
            self.open_local_directory(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::move_target_focus;

    #[test]
    fn target_focus_skips_disabled_machines_and_has_nowhere_to_go_if_all_disabled() {
        assert_eq!(move_target_focus(&[], None, "down"), None);
        assert_eq!(move_target_focus(&[1, 3], None, "down"), Some(1));
        assert_eq!(move_target_focus(&[1, 3], Some(1), "down"), Some(3));
        assert_eq!(move_target_focus(&[1, 3], Some(3), "down"), Some(1));
        assert_eq!(move_target_focus(&[1, 3], Some(1), "up"), Some(3));
    }
}
