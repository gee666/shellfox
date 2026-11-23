//! The heading row of one peer section: hostname, count, connection state and action.
use super::*;

impl Root {
    pub(super) fn peer_heading(
        &self,
        cycle: &source::PeerCycle,
        state: &source::PeerState,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let peer = &cycle.peer;
        let connected = state.phase == source::Phase::Connected && state.snapshot.is_some();
        let expanded = !self.peer_ui.collapsed.contains(peer);
        let selected = self.peers.selected == source::Source::Peer(peer.clone());
        let (label, detail) = connection_label(state);
        use source::ConnectionAction;
        let action = state.connection_action();
        let active = matches!(
            action,
            Some(ConnectionAction::Cancel(_) | ConnectionAction::Disconnect)
        );
        let Some(heading_focus) = self
            .peer_ui
            .sidebar_layout
            .borrow()
            .get(peer)
            .map(|layout| layout.heading.clone())
        else {
            return div().into_any_element();
        };
        let count = connected.then(|| {
            let total = state
                .snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.sessions.len());
            format!("· {total}")
        });
        let hostname = state.hostname.clone();
        let target = peer.clone();
        let toggle = peer.clone();
        let host = state.hostname.as_str();
        let button_tip = match action {
            Some(ConnectionAction::Cancel(_)) => "Cancel request".into(),
            Some(ConnectionAction::Disconnect) => format!("{detail} Disconnect"),
            Some(ConnectionAction::Retry) => format!("{detail} Request access again"),
            Some(ConnectionAction::Connect) => format!("Connect to {host}"),
            None => detail.clone(),
        };
        let disabled = action.is_none();
        div()
            .id(gpui::SharedString::from(format!(
                "peer-heading-{}",
                peer.as_str()
            )))
            .h(px(29.))
            .when(connected, |heading| heading.h(px(33.)).pt(px(4.)))
            .border_t_1()
            .border_color(palette.border)
            .flex()
            .items_center()
            .gap(px(6.))
            .pr(px(6.))
            .child(
                div()
                    .id("peer-section-toggle")
                    .track_focus(&heading_focus)
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(9.))
                    .text_size(px(11.))
                    .text_color(if selected {
                        palette.foreground
                    } else {
                        palette.muted
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(palette.hover))
                    .child(
                        icon_sized(if expanded { "down" } else { "chevron" }, 12.).flex_shrink_0(),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .relative()
                            .top(px(1.))
                            .child(div().min_w_0().child(inline_ellipsis(hostname, 14.)))
                            .children(count.map(|count| div().flex_shrink_0().child(count))),
                    )
                    .on_click(cx.listener(move |root, _, window, cx| {
                        if !root.peer_ui.collapsed.remove(&target) {
                            root.peer_ui.collapsed.insert(target.clone());
                        }
                        if let Err(error) = root.peers.choose(source::Source::Peer(target.clone()))
                        {
                            root.error = Some(error.to_string());
                        }
                        root.hide_settings(cx);
                        root.peer_ui.pending_create = None;
                        root.resume_peer_selection(&target, window, cx);
                        cx.notify();
                    })),
            )
            .child(
                self.semantic_region(
                    &format!("sidebar/{}/connection", peer.as_str()),
                    crate::accessibility::Role::Image,
                    label,
                    div()
                        .id("peer-connection-dot")
                        .size(px(5.))
                        .rounded_full()
                        .bg(connection_color(state, palette))
                        .tooltip(tooltip(detail, palette)),
                ),
            )
            .child(
                icon_button(
                    format!("peer-connection-{:?}", state.phase),
                    if active { "disconnect" } else { "connect" },
                    palette,
                )
                .size(px(24.))
                .tooltip(tooltip(button_tip, palette))
                .when(disabled, |button| button.opacity(0.4))
                .when_some(action, |button, action| {
                    button.on_click(cx.listener(move |root, _, window, cx| match action {
                        ConnectionAction::Cancel(id) => {
                            root.cancel_peer_pair(toggle.clone(), id, window, cx)
                        }
                        ConnectionAction::Disconnect => {
                            root.disconnect_peer(toggle.clone(), window, cx)
                        }
                        ConnectionAction::Connect | ConnectionAction::Retry => {
                            root.connect_peer(toggle.clone(), window, cx)
                        }
                    }))
                }),
            )
            .into_any_element()
    }
}
