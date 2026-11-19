use super::*;
use gpui::{div, prelude::*, px, rgb, AnyElement};

impl TerminalView {
    pub(in crate::app) fn peer_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Backend::Peer(peer) = &self.backend else {
            return None;
        };
        // The workspace owns machine-wide connection states. This overlay is
        // only for a terminal on a connected machine that has not attached yet.
        if peer.binding.state().status != ConnectionStatus::Connected {
            return None;
        }
        let palette = crate::theme::Palette::new(
            &format!("#{:06x}", self.theme.accent),
            &format!("#{:06x}", self.theme.background),
        );
        let panel = || {
            div()
                .id("peer-terminal-status")
                .absolute()
                .inset_0()
                .bg(rgb(self.theme.background))
                .text_color(palette.muted)
                .text_size(px(12.))
        };
        let action = |label, cx: &mut Context<Self>| {
            crate::chrome::button("peer-terminal-action", label, palette).on_click(cx.listener(
                |view, _, window, cx| {
                    if let Some(peer) = view.peer_mut() {
                        if matches!(peer.state, State::Unavailable(_)) {
                            peer.state = State::Offline;
                            view.poll_peer(cx);
                        } else {
                            view.take_peer_control(cx);
                        }
                    }
                    view.focus.focus(window);
                    cx.notify();
                },
            ))
        };
        let centered = |text: &'static str| {
            panel()
                .occlude()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(text)
        };
        match &peer.state {
            State::Offline | State::Attaching(_) => Some(
                panel()
                    .child(crate::renderer::terminal_loading(palette.accent))
                    .into_any_element(),
            ),
            State::Blocked(_) => Some(
                centered("Another client is controlling this terminal")
                    .child(action("Take it", cx))
                    .into_any_element(),
            ),
            State::Unavailable(error) => Some(
                div()
                    .absolute()
                    .inset_0()
                    .child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .right(px(12.))
                            .child(action("Reconnect terminal", cx)),
                    )
                    .child(
                        div()
                            .absolute()
                            .bottom(px(8.))
                            .right(px(12.))
                            .text_size(px(11.))
                            .text_color(palette.muted)
                            .bg(rgb(self.theme.background))
                            .child(error.clone()),
                    )
                    .into_any_element(),
            ),
            State::Attached { .. } => None,
        }
    }
}
