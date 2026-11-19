use super::*;
use crate::{
    assets::{ellipsis, icon_button, icon_sized, tooltip},
    sidebar::row::{shorten_home, status_label},
    tab_activity::status_dot,
};
use gpui::{Div, Stateful};
#[path = "peer_sidebar_row.rs"]
mod row;
use shellfox_protocol::{Status, TabLifecycle, TabStatus};
use shellfox_service::activity::AgentState;

pub(super) fn settled(session: &m::Session) -> bool {
    session
        .facts
        .settled_at
        .known()
        .is_some_and(Option::is_some)
        || matches!(session.facts.status.known(), Some(Status::Settled))
}

pub(super) fn error_text(session: &m::Session, closed: bool) -> Option<&str> {
    session
        .facts
        .error
        .known()
        .and_then(Option::as_ref)
        .map(|error| error.message.as_str())
        .or_else(|| {
            session
                .tabs
                .iter()
                .filter(|tab| {
                    closed || !matches!(tab.facts.lifecycle.known(), Some(TabLifecycle::Closed))
                })
                .find_map(|tab| {
                    tab.facts
                        .error
                        .known()
                        .and_then(Option::as_ref)
                        .map(|error| error.message.as_str())
                })
        })
}

pub(super) fn has_error(session: &m::Session) -> bool {
    error_text(session, true).is_some()
        || matches!(session.facts.status.known(), Some(Status::Error))
        || session
            .tabs
            .iter()
            .any(|tab| matches!(tab.facts.status.known(), Some(TabStatus::Error)))
}

fn dot(session: &m::Session, busy: impl Fn(m::Id) -> bool) -> AgentState {
    if settled(session) {
        return AgentState::Settled;
    }
    let mut running = false;
    let mut working = false;
    if error_text(session, false).is_some() {
        return AgentState::Error;
    }
    for tab in &session.tabs {
        if matches!(tab.facts.lifecycle.known(), Some(TabLifecycle::Closed)) {
            continue;
        }
        match tab.facts.status.known() {
            Some(TabStatus::Error) => return AgentState::Error,
            Some(TabStatus::Running) => {
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

fn connection_label(peer: &source::PeerState) -> (&'static str, String, bool) {
    use source::Phase;
    let host = &peer.hostname;
    match peer.phase {
        Phase::Connected => ("Connected", format!("Connected to {host}"), true),
        Phase::AwaitingApproval => (
            "Waiting for approval",
            format!("Approve this request in Shellfox on {host}."),
            true,
        ),
        Phase::Connecting => ("Connecting", format!("Connecting to {host}…"), true),
        Phase::Reconnecting => (
            "Reconnecting",
            peer.error
                .clone()
                .unwrap_or_else(|| format!("Reconnecting to {host}…")),
            true,
        ),
        Phase::Rejected => (
            "Access denied",
            peer.error.clone().unwrap_or_else(|| {
                "The host declined the request. You can request access again.".into()
            }),
            false,
        ),
        Phase::Disconnected if peer.online => ("Disconnected", "Ready to connect".into(), false),
        Phase::Disconnected => ("Offline", "This machine is offline.".into(), false),
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
    pub(crate) fn peer_sidebar(
        &self,
        mut list: Stateful<Div>,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut peers: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter(|(_, state)| state.remembered)
            .collect();
        peers.sort_by(|a, b| a.1.hostname.cmp(&b.1.hostname));
        let mut y = self.first_peer_section_y();
        for (peer, state) in peers {
            let connected = state.phase == source::Phase::Connected && state.snapshot.is_some();
            let expanded = !self.peer_ui.collapsed.contains(peer);
            let selected = self.peers.selected == source::Source::Peer(peer.clone());
            let cycle = source::PeerCycle {
                peer: peer.clone(),
                generation: state.cycle,
            };
            let (label, detail, active) = connection_label(state);
            let mut rows: Vec<_> = state
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    source::sessions(snapshot)
                        .map(|session| {
                            (
                                session,
                                dot(session, |tab| {
                                    self.peer_tab_busy(peer, tab).unwrap_or(false)
                                }),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            rows.sort_by(|(a, a_dot), (b, b_dot)| {
                let a_pin = a.facts.pinned_at.known().and_then(Option::as_ref);
                let b_pin = b.facts.pinned_at.known().and_then(Option::as_ref);
                b_pin
                    .is_some()
                    .cmp(&a_pin.is_some())
                    .then_with(|| a_pin.cmp(&b_pin))
                    .then_with(|| crate::sidebar::rank(*a_dot).cmp(&crate::sidebar::rank(*b_dot)))
                    .then_with(|| a.facts.created_at.known().cmp(&b.facts.created_at.known()))
                    .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
            });
            let header_height = if connected { 33. } else { 29. };
            y += header_height;
            let shown = if connected && expanded {
                rows.len().min(
                    self.peer_ui
                        .pages
                        .get(peer)
                        .copied()
                        .unwrap_or(1)
                        .saturating_mul(self.history_page_size),
                )
            } else {
                0
            };
            let more = connected && expanded && shown < rows.len();
            let heading_focus = self.record_peer_layout(
                &cycle,
                y,
                rows[..shown]
                    .iter()
                    .map(|(session, _)| session.id)
                    .collect(),
                more,
                cx,
            );
            let heading = if connected {
                format!("{} · {}", state.hostname, rows.len())
            } else {
                state.hostname.clone()
            };
            let target = peer.clone();
            let toggle = peer.clone();
            let host = state.hostname.as_str();
            let button_tip = if active {
                format!("{detail} Disconnect")
            } else if state.phase == source::Phase::Rejected {
                format!("{detail} Request access again")
            } else {
                format!("Connect to {host}")
            };
            let disabled = state.pending || !state.online && !active;
            list = list.child(
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
                                    .truncate()
                                    .relative()
                                    .top(px(1.))
                                    .child(heading),
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
                                root.settings = false;
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
                        .when(!disabled, |button| {
                            button.on_click(cx.listener(move |root, _, window, cx| {
                                if active {
                                    root.disconnect_peer(toggle.clone(), window, cx);
                                } else {
                                    root.connect_peer(toggle.clone(), false, window, cx);
                                }
                            }))
                        }),
                    ),
            );
            if connected && expanded {
                if rows.is_empty() {
                    list = list.child(
                        div()
                            .mx(px(8.))
                            .my(px(12.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(palette.muted)
                            .child("No sessions, press +"),
                    );
                    y += 40.;
                }
                list = self.render_peer_page(list, &cycle, &rows[..shown], palette, window, cx);
                y += shown as f32 * crate::sidebar::windowed::ROW_HEIGHT;
                if more {
                    y += 32.;
                }
            }
            if connected {
                list = list.child(div().h(px(4.)));
                y += 4.;
            }
        }
        list
    }
}
