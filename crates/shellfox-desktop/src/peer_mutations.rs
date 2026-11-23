//! Original-origin peer mutations; UI changes never pull a user back from another source.
use super::*;
impl Root {
    pub(super) fn add_peer_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (cycle, api) = match self.selected_peer_api() {
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
        let session_id = session;
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
                let before = api.snapshot().await?;
                api.call(
                    shellfox_protocol::AddTab {
                        session_id,
                        title: None,
                        profile_id: None,
                        cwd: None,
                    },
                    |_| (),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.message))?;
                let old: HashSet<_> = before
                    .sessions
                    .iter()
                    .flat_map(|session| session.tabs.iter().map(|tab| tab.id))
                    .collect();
                let snapshot = api.snapshot().await?;
                let tab = snapshot
                    .sessions
                    .iter()
                    .find(|value| value.id == session)
                    .and_then(|value| value.tabs.iter().find(|tab| !old.contains(&tab.id)))
                    .map(|tab| tab.id);
                anyhow::Ok((tab, snapshot))
            }
            .await;
            let _ = root.update_in(cx, |root, _window, cx| {
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
