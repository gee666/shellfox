use super::*;
use crate::{
    assets::{ellipsis, icon_button, icon_sized, inline_ellipsis, tooltip},
    sidebar::row::{shorten_home, status_label},
    tab_activity::status_dot,
};
use crate::sidebar::list::Entry;
#[path = "peer_sidebar_row.rs"]
mod row;
use shellfox_protocol::{SessionDto, Status, TabId, TabLifecycle, TabStatus};
use shellfox_service::activity::AgentState;

pub(super) fn settled(session: &SessionDto) -> bool {
    session.settled_at.is_some() || matches!(session.status, Status::Settled)
}

pub(super) fn error_text(session: &SessionDto, closed: bool) -> Option<&str> {
    session
        .error
        .as_ref()
        .map(|error| error.message.as_str())
        .or_else(|| {
            session
                .tabs
                .iter()
                .filter(|tab| {
                    closed || !matches!(tab.lifecycle, TabLifecycle::Closed)
                })
                .find_map(|tab| {
                    tab.error
                        .as_ref()
                        .map(|error| error.message.as_str())
                })
        })
}

pub(super) fn has_error(session: &SessionDto) -> bool {
    error_text(session, true).is_some()
        || matches!(session.status, Status::Error)
        || session
            .tabs
            .iter()
            .any(|tab| matches!(tab.status, TabStatus::Error))
}

fn dot(session: &SessionDto, busy: impl Fn(TabId) -> bool) -> AgentState {
    if settled(session) {
        return AgentState::Settled;
    }
    let mut running = false;
    let mut working = false;
    if error_text(session, false).is_some() {
        return AgentState::Error;
    }
    for tab in &session.tabs {
        if matches!(tab.lifecycle, TabLifecycle::Closed) {
            continue;
        }
        match tab.status {
            TabStatus::Error => return AgentState::Error,
            TabStatus::Running => {
                running = true;
                working |= busy(tab.id);
            }
            _ => {}
        }
    }
    if working {
        AgentState::Busy
    } else if running {
        AgentState::Running
    } else {
        AgentState::Shell
    }
}

fn connection_label(peer: &source::PeerState) -> (&'static str, String) {
    use source::Phase;
    let host = &peer.hostname;
    match peer.phase {
        Phase::Connected => ("Connected", format!("Connected to {host}")),
        Phase::AwaitingApproval => (
            "Waiting for approval",
            format!("Approve this request in Shellfox on {host}."),
        ),
        Phase::Connecting => ("Connecting", format!("Connecting to {host}…")),
        Phase::Reconnecting => (
            "Reconnecting",
            peer.error
                .clone()
                .unwrap_or_else(|| format!("Reconnecting to {host}…")),
        ),
        Phase::Rejected => (
            "Access denied",
            peer.error.clone().unwrap_or_else(|| {
                "The host declined the request. You can request access again.".into()
            }),
        ),
        Phase::Disconnected if peer.online => ("Disconnected", "Ready to connect".into()),
        Phase::Disconnected => ("Offline", "This machine is offline.".into()),
    }
}

pub(crate) fn connection_color(peer: &source::PeerState, palette: Palette) -> gpui::Hsla {
    use source::Phase;
    match peer.phase {
        Phase::Connected => palette.status_color(0x4ade80),
        Phase::Connecting | Phase::AwaitingApproval | Phase::Reconnecting => palette.accent,
        Phase::Rejected => palette.status_color(0xfc8397),
        Phase::Disconnected => palette.muted.opacity(0.55),
    }
}

impl Root {
    fn peer_rows<'a>(
        &self,
        peer: &AuthenticatedPeerId,
        state: &'a source::PeerState,
    ) -> Vec<(&'a SessionDto, AgentState)> {
        let mut rows: Vec<_> = state
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .sessions
                    .iter()
                    .map(|session| {
                        (session, dot(session, |tab| self.peer_tab_busy(peer, tab).unwrap_or(false)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.sort_by(|(a, a_dot), (b, b_dot)| {
            let a_pin = a.pinned_at.as_ref();
            let b_pin = b.pinned_at.as_ref();
            b_pin
                .is_some()
                .cmp(&a_pin.is_some())
                .then_with(|| a_pin.cmp(&b_pin))
                .then_with(|| crate::sidebar::rank(*a_dot).cmp(&crate::sidebar::rank(*b_dot)))
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        rows
    }

    /// Appends one section per remembered peer: heading, shown rows, "Show more".
    pub(crate) fn peer_entries(&self, entries: &mut Vec<Entry>, window: &Window, cx: &mut gpui::App) {
        let mut peers: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter(|(_, state)| state.remembered)
            .collect();
        peers.sort_by(|a, b| a.1.hostname.cmp(&b.1.hostname));
        for (peer, state) in peers {
            let connected = state.phase == source::Phase::Connected && state.snapshot.is_some();
            let expanded = connected && !self.peer_ui.collapsed.contains(peer);
            let rows = self.peer_rows(peer, state);
            let pages = self.peer_ui.pages.get(peer).copied().unwrap_or(1);
            let shown = if expanded {
                rows.len().min(pages.saturating_mul(self.history_page_size))
            } else {
                0
            };
            let more = expanded && shown < rows.len();
            let cycle = source::PeerCycle {
                peer: peer.clone(),
                generation: state.cycle,
            };
            let ids: Vec<_> = rows[..shown].iter().map(|(session, _)| session.id).collect();
            entries.push(Entry::PeerHeader(peer.clone()));
            if expanded && rows.is_empty() {
                entries.push(Entry::PeerEmpty(peer.clone()));
            }
            entries.extend(ids.iter().map(|id| Entry::Peer(peer.clone(), *id)));
            if more {
                entries.push(Entry::PeerMore(peer.clone()));
            }
            if connected {
                entries.push(Entry::PeerEnd(peer.clone()));
            }
            self.record_peer_layout(&cycle, ids, more, window, cx);
        }
    }

    pub(crate) fn render_peer_entry(
        &self,
        entry: &Entry,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (Entry::PeerHeader(peer)
        | Entry::PeerEmpty(peer)
        | Entry::Peer(peer, _)
        | Entry::PeerMore(peer)
        | Entry::PeerEnd(peer)) = entry
        else {
            return div().into_any_element();
        };
        let Some(state) = self.peers.peers.get(peer) else {
            return div().into_any_element();
        };
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        match entry {
            Entry::PeerHeader(_) => self.peer_heading(&cycle, state, palette, cx),
            Entry::PeerEmpty(_) => crate::sidebar::list::empty_sessions(palette).into_any_element(),
            Entry::Peer(_, id) => {
                let Some((session, dot)) = self
                    .peer_rows(peer, state)
                    .into_iter()
                    .find(|(session, _)| session.id == *id)
                else {
                    return div().into_any_element();
                };
                self.render_peer_row(&cycle, session, dot, palette, window, cx)
            }
            Entry::PeerMore(_) => self.render_peer_more(&cycle, palette, cx),
            _ => div().h(px(4.)).into_any_element(),
        }
    }

    fn peer_heading(
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
        let active = matches!(action, Some(ConnectionAction::Cancel(_) | ConnectionAction::Disconnect));
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
            let total = state.snapshot.as_ref().map_or(0, |snapshot| snapshot.sessions.len());
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
                        icon_sized(if expanded { "down" } else { "chevron" }, 12.)
                            .flex_shrink_0(),
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
                        if let Err(error) =
                            root.peers.choose(source::Source::Peer(target.clone()))
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
                        ConnectionAction::Cancel(id) => root.cancel_peer_pair(toggle.clone(), id, window, cx),
                        ConnectionAction::Disconnect => root.disconnect_peer(toggle.clone(), window, cx),
                        ConnectionAction::Connect | ConnectionAction::Retry => root.connect_peer(toggle.clone(), window, cx),
                    }))
                }),
            )
            .into_any_element()
    }
}
