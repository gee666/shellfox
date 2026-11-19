use super::*;
use crate::session_menu::{Item, MenuSession};
use shellfox_protocol as wire;

pub(super) enum Mutation {
    Pin(bool),
    Rename(String),
    Environment(Vec<wire::EnvVar>),
    ClearError,
    Settle(bool),
    Delete,
    Activate,
}

impl Root {
    pub(crate) fn peer_session(&self, cycle: &source::PeerCycle, id: m::Id) -> Option<&m::Session> {
        if !self.peers.current(cycle) {
            return None;
        }
        self.peers
            .peers
            .get(&cycle.peer)?
            .snapshot
            .as_ref()
            .and_then(|snapshot| source::sessions(snapshot).find(|session| session.id == id))
    }

    pub(crate) fn peer_session_menu_info(
        &self,
        cycle: &source::PeerCycle,
        id: m::Id,
    ) -> Option<MenuSession> {
        let session = self.peer_session(cycle, id)?;
        Some(MenuSession {
            pinned: session.facts.pinned_at.known().is_some_and(Option::is_some),
            settled: sidebar::settled(session),
            has_error: sidebar::has_error(session),
            pending: self.peer_ui.pending.contains(&cycle.peer),
            local: false,
        })
    }

    pub(crate) fn run_peer_session_item(
        &mut self,
        cycle: source::PeerCycle,
        id: m::Id,
        item: Item,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(info) = self.peer_session_menu_info(&cycle, id) else {
            return;
        };
        if info.pending {
            return;
        }
        match item {
            Item::Pin => {
                self.mutate_peer_session(cycle, id, Mutation::Pin(!info.pinned), window, cx)
            }
            Item::Copy => {
                if let Some(cwd) = self
                    .peer_session(&cycle, id)
                    .and_then(|session| session.facts.cwd.known().cloned())
                {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(cwd));
                    self.notify_info(cx, "Path copied");
                }
            }
            Item::Settle => self.toggle_peer_settlement(cycle, id, info.settled, window, cx),
            Item::ClearError => {
                self.mutate_peer_session(cycle, id, Mutation::ClearError, window, cx)
            }
            Item::Rename | Item::Environment | Item::Delete => {
                self.open_peer_session_dialog(cycle, id, item, window, cx)
            }
            Item::Explorer => {}
        }
    }

    pub(crate) fn toggle_peer_settlement(
        &mut self,
        cycle: source::PeerCycle,
        id: m::Id,
        settled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mutate_peer_session(cycle, id, Mutation::Settle(!settled), window, cx);
    }

    pub(crate) fn activate_peer_session(
        &mut self,
        cycle: source::PeerCycle,
        id: m::Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.peer_session(&cycle, id) else {
            return;
        };
        let has_tabs = session.tabs.iter().any(|tab| {
            !matches!(
                tab.facts.lifecycle.known(),
                Some(wire::TabLifecycle::Closed)
            )
        });
        let external = session
            .facts
            .adapter_id
            .known()
            .is_some_and(|kind| !matches!(kind, wire::AdapterId::EmbeddedPty));
        self.select_peer_session(cycle.clone(), id, None, window, cx);
        if !external || !has_tabs {
            self.mutate_peer_session(cycle, id, Mutation::Activate, window, cx);
        }
    }

    pub(super) fn mutate_peer_session(
        &mut self,
        cycle: source::PeerCycle,
        id: m::Id,
        mutation: Mutation,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window
            || self.peer_ui.pending.contains(&cycle.peer)
            || self.peer_session(&cycle, id).is_none()
        {
            return;
        }
        let Ok(api) = self.peer_api(&cycle) else {
            return;
        };
        let Ok(session_id) = id.to_string().parse::<SessionId>() else {
            return;
        };
        self.peer_ui.pending.insert(cycle.peer.clone());
        let closes_dialog = matches!(
            &mutation,
            Mutation::Rename(_) | Mutation::Delete | Mutation::Environment(_)
        );
        let activates = matches!(&mutation, Mutation::Activate);
        let source_epoch = self.source_selection_epoch();
        cx.spawn_in(window, async move |root, cx| {
            macro_rules! perform {
                ($method:ident, $request:expr) => {{
                    let request = $request;
                    api.call(request.clone(), m::$method(request), |_| (), |_| ())
                        .await
                }};
            }
            let result = match mutation {
                Mutation::Pin(pinned) => perform!(
                    SetSessionPinned,
                    wire::SetSessionPinned { session_id, pinned }
                ),
                Mutation::Rename(title) => {
                    perform!(RenameSession, wire::RenameSession { session_id, title })
                }
                Mutation::Environment(env) => {
                    perform!(SetSessionEnv, wire::SetSessionEnv { session_id, env })
                }
                Mutation::ClearError => {
                    perform!(ClearSessionError, wire::ClearSessionError { session_id })
                }
                Mutation::Settle(true) => perform!(
                    SettleSession,
                    wire::SettleSession {
                        session_id,
                        confirm_active: false
                    }
                ),
                Mutation::Settle(false) => {
                    perform!(UnsettleSession, wire::UnsettleSession { session_id })
                }
                Mutation::Delete => perform!(DeleteSession, wire::DeleteSession { session_id }),
                Mutation::Activate => {
                    perform!(ActivateSession, wire::ActivateSession { session_id })
                }
            };
            let result = match result {
                Ok(()) => api.snapshot().await.map_err(|error| error.to_string()),
                Err(error) => Err(error.message),
            };
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) {
                    return;
                }
                root.peer_ui.pending.remove(&cycle.peer);
                match result {
                    Ok(snapshot) => {
                        if let Err(error) = root.peers.catalog(&cycle, snapshot) {
                            root.error = Some(error.to_string());
                        }
                        if closes_dialog {
                            root.dismiss_completed_peer_dialog(&cycle, id, window, cx);
                        }
                        if activates
                            && root.source_selection_epoch() == source_epoch
                            && root.peers.selected_peer().is_some_and(|(peer, state)| {
                                peer == &cycle.peer && state.selected_session == Some(id)
                            })
                        {
                            root.select_peer_session(cycle.clone(), id, None, window, cx);
                        }
                    }
                    Err(message) => root.error = Some(message),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
