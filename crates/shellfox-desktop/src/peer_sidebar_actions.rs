use super::*;
use crate::{
    session_menu::{Item, MenuSession},
    workspace::ActivationFence,
};
use shellfox_protocol as wire;

pub(super) enum Mutation {
    Pin(bool),
    Rename(String),
    Environment(Vec<wire::EnvVar>),
    ClearError,
    Settle(bool),
    Delete,
}

type ActivationKey = (source::PeerCycle, m::Id);

#[derive(Default)]
pub(super) struct PeerActivations(HashMap<ActivationKey, ActivationFence>);

impl PeerActivations {
    fn begin(&mut self, key: ActivationKey, fence: ActivationFence) -> bool {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.0.entry(key) {
            entry.insert(fence);
            true
        } else {
            false
        }
    }

    fn finish(&mut self, key: &ActivationKey, current: Option<ActivationFence>) -> bool {
        self.0.remove(key).is_some_and(|requested| Some(requested) == current)
    }

    pub(super) fn retire(&mut self, peer: &AuthenticatedPeerId) {
        self.0.retain(|(cycle, _), _| &cycle.peer != peer);
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;

    #[test]
    fn a_pending_row_deduplicates_only_itself_and_cannot_publish_after_a_switch() {
        let mut opening = PeerActivations::default();
        let cycle = source::PeerCycle {
            peer: AuthenticatedPeerId::new("owned-fixture").unwrap(),
            generation: 1,
        };
        let row = || SessionId::new().to_string().parse::<m::Id>().unwrap();
        let a = (cycle.clone(), row());
        let b = (cycle, row());
        let first = ActivationFence { source: 1, navigation: 1 };
        let second = ActivationFence { source: 1, navigation: 2 };
        assert!(opening.begin(a.clone(), first));
        assert!(!opening.begin(a.clone(), first));
        assert!(opening.begin(b.clone(), second));
        assert!(opening.finish(&b, Some(second)));
        assert!(!opening.finish(&a, None));

        assert!(opening.begin(a.clone(), first));
        let returned = ActivationFence { source: 1, navigation: 3 };
        assert!(!opening.finish(&a, Some(returned)));
        assert!(opening.begin(a.clone(), returned));
        assert!(opening.finish(&a, Some(returned)));
    }
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
        self.hide_settings(cx);
        self.peer_ui.pending_create = None;
        let key = (cycle.clone(), id);
        if let Some(current) = self.peer_activation_fence(&key) {
            if self.peer_ui.opening.0.get(&key) == Some(&current) {
                cx.notify();
                return;
            }
        }
        self.select_peer_session(cycle, id, None, window, cx);
        if !external || !has_tabs {
            self.request_peer_activation(key, window, cx);
        }
    }

    fn peer_activation_fence(&self, (cycle, session): &ActivationKey) -> Option<ActivationFence> {
        let (peer, state) = self.peers.selected_peer()?;
        (self.peers.current(cycle) && peer == &cycle.peer && state.selected_session == Some(*session))
            .then(|| ActivationFence {
                source: self.source_selection_epoch(),
                navigation: self.peer_ui.navigation.get(peer).copied().unwrap_or_default(),
            })
    }

    fn request_peer_activation(&mut self, key: ActivationKey, window: &Window, cx: &mut Context<Self>) {
        if self.closing_window {
            return;
        }
        let Some(fence) = self.peer_activation_fence(&key) else { return };
        let (cycle, id) = &key;
        let Ok(api) = self.peer_api(cycle) else { return };
        let Ok(session_id) = id.to_string().parse() else { return };
        if !self.peer_ui.opening.begin(key.clone(), fence) {
            return;
        }
        cx.spawn_in(window, async move |root, cx| {
            let request = wire::ActivateSession { session_id };
            let result = match api.call(request.clone(), m::ActivateSession(request), |_| (), |_| ()).await {
                Ok(()) => api.snapshot().await.map_err(|error| error.to_string()),
                Err(error) => Err(error.message),
            };
            let _ = root.update_in(cx, |root, window, cx| {
                let current = root.peer_activation_fence(&key);
                if !root.peer_ui.opening.finish(&key, current) || root.closing_window {
                    return;
                }
                let (cycle, id) = key;
                match result {
                    Ok(snapshot) => match root.peers.catalog(&cycle, snapshot) {
                        Ok(true) => root.select_peer_session(cycle, id, None, window, cx),
                        Ok(false) => {}
                        Err(error) => root.error = Some(error.to_string()),
                    },
                    Err(message) => root.notify_machine_error(&message, cx),
                }
                cx.notify();
            });
        }).detach();
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
