use super::*;
use crate::root::{
    peer::metadata::{PeerApi, PeerCallError},
    source::PeerCycle,
};
use shellfox_protocol::{self as wire, manager_v2 as m};

#[derive(Clone)]
pub(super) struct PeerDirectory {
    cycle: PeerCycle,
    api: PeerApi,
    pub(super) name: String,
    fallback_profile: Option<String>,
}

impl PeerDirectory {
    pub(super) async fn complete(&self, path: String) -> Result<Vec<String>, paths::PathError> {
        let request = wire::CompleteDirectory { path };
        self.api
            .call(
                request.clone(),
                m::CompleteDirectory(request),
                |value| value.matches,
                |value| value.matches,
            )
            .await
            .map_err(path_error)
    }

    pub(super) async fn resolve(
        &self,
        path: String,
        create: bool,
    ) -> Result<String, paths::PathError> {
        if create {
            let request = wire::CreateDirectory {
                path,
                session_id: None,
                tab_id: None,
            };
            self.api
                .call(
                    request.clone(),
                    m::CreateDirectory(request),
                    |value| value.cwd,
                    |value| value.cwd,
                )
                .await
        } else {
            let request = wire::ResolveDirectory { path };
            self.api
                .call(
                    request.clone(),
                    m::ResolveDirectory(request),
                    |value| value.cwd,
                    |value| value.cwd,
                )
                .await
        }
        .map_err(path_error)
    }
}

fn path_error(error: PeerCallError) -> paths::PathError {
    paths::PathError {
        missing: error.code == Some(wire::ErrorCode::NotFound),
        message: error.message,
    }
}

impl Root {
    pub(crate) fn open_peer_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, peer)) = self.peers.selected_peer() else {
            return;
        };
        let cycle = PeerCycle {
            peer: id.clone(),
            generation: peer.cycle,
        };
        let Ok(api) = self.peer_api(&cycle) else {
            return;
        };
        let Some(snapshot) = &peer.snapshot else {
            return;
        };
        // Native catalog reads may omit stale discovery facts. Embedded
        // CreateSession uses the host's separately validated profile catalog;
        // the createWindow bit is still required for external window adapters.
        let probe = snapshot.probe.known();
        let embedded =
            probe.is_none_or(|probe| matches!(probe.adapter_id, wire::AdapterId::EmbeddedPty));
        if !embedded
            && !probe.is_some_and(|probe| probe.available && probe.capabilities.create_window)
        {
            return;
        }
        let home = peer.home.as_ref().map(PathBuf::from);
        let name = peer.hostname.clone();
        let windows = probe.is_some_and(|probe| probe.platform == "win32");
        let platform_known = probe.is_some();
        let configured = snapshot
            .settings
            .known()
            .and_then(|settings| settings.terminal_profile_id.clone().flatten());
        let epoch = self.source_selection_epoch();
        if !self.peer_ui.pending.insert(cycle.peer.clone()) {
            return;
        }
        cx.spawn_in(window, async move |root, cx| {
            let profile = if embedded {
                api.call(
                    wire::GetTerminalProfiles {},
                    m::GetTerminalProfiles(wire::GetTerminalProfiles {}),
                    |value| value,
                    |value| value,
                )
                .await
                .and_then(|catalog| {
                    let profile = configured.or(catalog.default_profile_id);
                    if catalog.profiles.iter().any(|item| {
                        item.available && (windows || Some(&item.id) == profile.as_ref())
                    }) {
                        Ok(if windows || !platform_known {
                            None
                        } else {
                            profile
                        })
                    } else {
                        Err(PeerCallError {
                            message: "No discovered interactive terminal profile is available."
                                .into(),
                            code: None,
                        })
                    }
                })
            } else {
                Ok(configured.filter(|_| !windows))
            };
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) {
                    return;
                }
                root.peer_ui.pending.remove(&cycle.peer);
                if epoch != root.source_selection_epoch() {
                    return;
                }
                match profile {
                    Ok(fallback_profile) => root.show_directory(
                        Some(PeerDirectory {
                            cycle,
                            api,
                            name,
                            fallback_profile,
                        }),
                        home,
                        window,
                        cx,
                    ),
                    Err(error) => root.error = Some(error.message),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn create_peer_directory_session(
        &mut self,
        cwd: String,
        target: PeerDirectory,
        id: SessionId,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let cycle = target.cycle.clone();
        if !self.peers.current(&cycle) || !self.peer_ui.pending.insert(cycle.peer.clone()) {
            return;
        }
        let source_epoch = self.source_selection_epoch();
        let request = wire::CreateSession {
            title: Some(session_title(&cwd)),
            cwd,
            request_id: SessionId::new(),
        };
        cx.spawn_in(window, async move |root, cx| {
            let result = async {
                let (session, has_tabs) = target.api.call(request.clone(), m::CreateSession(request),
                    |session| anyhow::Ok((session.id.to_string().parse()?, session.tabs.iter().any(|tab| !matches!(tab.lifecycle, wire::TabLifecycle::Closed)))),
                    |entry| {
                        let has_tabs = matches!(&entry, m::CatalogEntry::Session { value } if value.tabs.iter().any(|tab| !matches!(tab.facts.lifecycle.known(), Some(wire::TabLifecycle::Closed))));
                        anyhow::Ok((entry.id(), has_tabs))
                    }).await.map_err(|error| anyhow::anyhow!(error.message))??;
                let tab_error = if has_tabs { None } else {
                    let request = wire::AddTab { session_id: session.to_string().parse()?, title: None, cwd: None, profile_id: target.fallback_profile };
                    target.api.call(request.clone(), m::AddTab(request), |_| (), |_| ()).await.err().map(|error| error.message)
                };
                let snapshot = target.api.snapshot().await?;
                anyhow::Ok((session, snapshot, tab_error))
            }
            .await;
            let _ = root.update_in(cx, |root, _window, cx| {
                if !root.peers.current(&cycle) {
                    return;
                }
                root.peer_ui.pending.remove(&cycle.peer);
                let current = root.source_selection_epoch() == source_epoch;
                let same_dialog =
                    matches!(&root.dialog, Some(Dialog::Create(draft)) if draft.id == id);
                match result {
                    Ok((session, snapshot, tab_error)) => {
                        if let Some(error) = tab_error { root.error = Some(error); }
                        if let Err(error) = root.peers.catalog(&cycle, snapshot) {
                            root.error = Some(error.to_string());
                        } else if current && same_dialog {
                            root.dialog = None;
                            root.select_peer_session(cycle.clone(), session, None, _window, cx);
                        }
                    }
                    Err(error) => {
                        if let Some(Dialog::Create(draft)) = &mut root.dialog
                            && draft.id == id
                        {
                            draft.busy = None;
                            draft.error = Some(error.to_string());
                            draft.path.read(cx).focus(_window);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

pub(super) fn session_title(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|title| !title.is_empty())
        .unwrap_or(cwd)
        .to_owned()
}
