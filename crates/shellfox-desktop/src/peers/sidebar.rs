use super::*;
use crate::sidebar::list::Entry;
use crate::{
    assets::{ellipsis, icon_button, icon_sized, inline_ellipsis, tooltip},
    sidebar::row::{shorten_home, status_label},
    workspace::tabs::activity::status_dot,
};
mod heading;
mod row;
#[cfg(test)]
mod tests;
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
                .filter(|tab| closed || !matches!(tab.lifecycle, TabLifecycle::Closed))
                .find_map(|tab| tab.error.as_ref().map(|error| error.message.as_str()))
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

fn sort_rows(rows: &mut [(&SessionDto, AgentState)]) {
    rows.sort_by(|(a, a_dot), (b, b_dot)| {
        shellfox_core::compare_session_order(&peer_order(a, *a_dot), &peer_order(b, *b_dot))
    });
}

fn peer_order(s: &SessionDto, dot: AgentState) -> shellfox_core::SessionOrder<'_> {
    crate::sidebar::session_order(s.pinned_at.is_some(), dot, &s.created_at, &s.title, s.id)
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
        sort_rows(&mut rows);
        rows
    }

    /// Appends one section per remembered peer: heading, shown rows, "Show more".
    pub(crate) fn peer_entries(
        &self,
        entries: &mut Vec<Entry>,
        window: &Window,
        cx: &mut gpui::App,
    ) {
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
                rows.len()
                    .min(pages.saturating_mul(self.sidebar.history_page_size))
            } else {
                0
            };
            let more = expanded && shown < rows.len();
            let cycle = source::PeerCycle {
                peer: peer.clone(),
                generation: state.cycle,
            };
            let ids: Vec<_> = rows[..shown]
                .iter()
                .map(|(session, _)| session.id)
                .collect();
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
}
