//! Original-origin peer mutations; UI changes never pull a user back from another source.
use super::*;
impl Root {
    pub(super) fn create_peer_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (cycle, connection, admission) = match self.peer_context() {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        if self.peer_ui.pending.contains(&cycle.peer) {
            return;
        }
        let Some((path, title)) = self.peer_ui.forms.get(&cycle.peer) else {
            return;
        };
        let title = title.read(cx).text.clone();
        let cwd = path.read(cx).text.clone();
        let request = shellfox_protocol::CreateSession {
            cwd,
            title: (!title.is_empty()).then_some(title),
            request_id: SessionId::new(),
        };
        let source_epoch = self.source_selection_epoch();
        let navigation = self
            .peer_ui
            .navigation
            .get(&cycle.peer)
            .copied()
            .unwrap_or(0);
        self.peer_ui.pending.insert(cycle.peer.clone());
        cx.spawn_in(window, async move |root, cx| {
            let result = async {
                let entry = connection
                    .execute_admitted(&admission, &m::CreateSession(request))
                    .await?;
                let id = entry.id();
                let snapshot = connection
                    .execute_admitted(
                        &admission,
                        &m::GetSnapshot(shellfox_protocol::GetSnapshot {}),
                    )
                    .await?;
                anyhow::Ok((id, snapshot))
            }
            .await;
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) {
                    return;
                }
                root.peer_ui.pending.remove(&cycle.peer);
                match result {
                    Ok((id, snapshot)) => {
                        if let Err(error) = root.peers.catalog(&cycle, snapshot).and_then(|_| {
                            if root.source_selection_epoch() == source_epoch
                                && root
                                    .peer_ui
                                    .navigation
                                    .get(&cycle.peer)
                                    .copied()
                                    .unwrap_or(0)
                                    == navigation
                                && !root.is_local_source()
                            {
                                root.peers.select(&cycle, id, None)?;
                            }
                            Ok(())
                        }) {
                            root.error = Some(error.to_string());
                        } else {
                            root.ensure_peer_terminal(window, cx);
                        }
                    }
                    Err(error) => root.error = Some(format!("Remote create failed: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn add_peer_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (cycle, connection, admission) = match self.peer_context() {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        if self.peer_ui.pending.contains(&cycle.peer) {
            return;
        }
        let Some(session) = self.peers.peers[&cycle.peer].selected_session else {
            return;
        };
        let session_id = match session.to_string().parse() {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(format!("Invalid remote session: {error}"));
                return;
            }
        };
        let source_epoch = self.source_selection_epoch();
        let navigation = self
            .peer_ui
            .navigation
            .get(&cycle.peer)
            .copied()
            .unwrap_or(0);
        self.peer_ui.pending.insert(cycle.peer.clone());
        cx.spawn_in(window, async move |root, cx| {
            let result = async {
                let before = connection
                    .execute_admitted(
                        &admission,
                        &m::GetSnapshot(shellfox_protocol::GetSnapshot {}),
                    )
                    .await?;
                let entry = connection
                    .execute_admitted(
                        &admission,
                        &m::AddTab(shellfox_protocol::AddTab {
                            session_id,
                            title: None,
                            profile_id: None,
                            cwd: None,
                        }),
                    )
                    .await?;
                let old: HashSet<_> = source::sessions(&before)
                    .flat_map(|session| session.tabs.iter().map(|tab| tab.id))
                    .collect();
                let tab = match entry {
                    m::CatalogEntry::Session { value } => value
                        .tabs
                        .iter()
                        .find(|tab| !old.contains(&tab.id))
                        .map(|tab| tab.id),
                    _ => None,
                };
                let snapshot = connection
                    .execute_admitted(
                        &admission,
                        &m::GetSnapshot(shellfox_protocol::GetSnapshot {}),
                    )
                    .await?;
                anyhow::Ok((tab, snapshot))
            }
            .await;
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) {
                    return;
                }
                root.peer_ui.pending.remove(&cycle.peer);
                match result {
                    Ok((tab, snapshot)) => {
                        if let Err(error) = root.peers.catalog(&cycle, snapshot).and_then(|_| {
                            if root.source_selection_epoch() == source_epoch
                                && root
                                    .peer_ui
                                    .navigation
                                    .get(&cycle.peer)
                                    .copied()
                                    .unwrap_or(0)
                                    == navigation
                                && !root.is_local_source()
                            {
                                root.peers.select(&cycle, session, tab)?;
                            }
                            Ok(())
                        }) {
                            root.error = Some(error.to_string());
                        } else {
                            root.ensure_peer_terminal(window, cx);
                        }
                    }
                    Err(error) => {
                        root.error = Some(format!("Remote tab creation failed: {error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
