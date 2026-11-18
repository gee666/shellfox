//! Owner construction, original-origin admission and view publication.
use super::*;
impl Root {
    pub(super) fn open_peer_terminal(
        &mut self,
        cycle: source::PeerCycle,
        tab: m::Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.peer_ui.frozen
            || !self.peers.current(&cycle)
            || self.peer_ui.attaching.contains(&cycle.peer)
        {
            return;
        }
        if let Some(view) = self.peer_ui.views.get(&(cycle.peer.clone(), tab)).cloned() {
            view.update(cx, |view, cx| {
                view.visible(true);
                view.focus(window);
                cx.notify();
            });
            return;
        }
        if self
            .peer_ui
            .views
            .keys()
            .filter(|(peer, _)| peer == &cycle.peer)
            .count()
            >= 32
        {
            self.error = Some("This peer already has 32 open terminal views.".into());
            return;
        }
        let Some(host) = self.host.as_ref() else {
            return;
        };
        let connection = match host.peer_existing(&cycle.peer) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let Some(witness) = self.peers.peers[&cycle.peer].origin.as_ref() else {
            return;
        };
        let admission = match connection.admit_source_with(witness) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let Some(snapshot) = self.peers.peers[&cycle.peer].snapshot.as_ref() else {
            return;
        };
        let control = match crate::host_client::peer::attach_control(snapshot, tab) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let prefs = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.preferences.clone())
            .unwrap_or_default();
        let (view, mut sdk) =
            match terminal::PeerTerminalView::create(connection, cycle.clone(), &prefs, window, cx)
            {
                Ok(value) => value,
                Err(error) => {
                    self.error = Some(error.to_string());
                    return;
                }
            };
        let stopped = self.pause_peer_poll(&cycle.peer);
        self.peer_ui.attaching.insert(cycle.peer.clone());
        self.peer_ui
            .views
            .insert((cycle.peer.clone(), tab), view.clone());
        let focus_before = window.focused(cx);
        cx.spawn_in(window, async move |root, cx| {
            if let Some(stopped) = stopped
                && stopped.await.is_err()
            {
                let _ = root.update_in(cx, |root, _, cx| {
                    root.fail_peer_cycle(
                        &cycle,
                        "Event poll outcome unknown; reconnect for fresh native recovery.".into(),
                        cx,
                    )
                });
                return;
            }
            if admission.check().is_err() {
                let _ = root.update_in(cx, |root, _, cx| {
                    root.fail_peer_cycle(
                        &cycle,
                        "Original peer binding retired while waiting for event drain.".into(),
                        cx,
                    )
                });
                return;
            }
            let result = sdk
                .attach(m::AttachTerminal {
                    tab_id: tab,
                    resume: None,
                    control,
                })
                .await;
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) || root.closing_window {
                    return;
                }
                root.peer_ui.attaching.remove(&cycle.peer);
                root.peer_ui.paused.remove(&cycle.peer);
                let visible = root.peers.selected_peer().is_some_and(|(peer, state)| {
                    peer == &cycle.peer && state.selected_tab == Some(tab)
                });
                let settled = root.peers.peers[&cycle.peer]
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| crate::host_client::peer::settlement(snapshot, tab));
                view.update(cx, |view, cx| {
                    view.hints.settled_at = settled;
                    view.visible(visible);
                    view.publish(sdk, result, cx);
                    if visible && window.focused(cx) == focus_before {
                        view.focus(window);
                    }
                });
                root.ensure_peer_terminal(window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn take_peer_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((peer, state)) = self.peers.selected_peer() else {
            return;
        };
        if self.peer_ui.frozen || self.peer_ui.attaching.contains(peer) {
            self.error = Some("Wait for this peer's recovery to finish.".into());
            return;
        }
        let Some(tab) = state.selected_tab else {
            return;
        };
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        let Some(view) = self.peer_ui.views.get(&(peer.clone(), tab)).cloned() else {
            return;
        };
        let mut sdk = match view.update(cx, |view, _| view.take_for_control()) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let stopped = self.pause_peer_poll(&cycle.peer);
        self.peer_ui.attaching.insert(cycle.peer.clone());
        cx.spawn_in(window, async move |root, cx| {
            if let Some(stopped) = stopped
                && stopped.await.is_err()
            {
                let _ = root.update_in(cx, |root, _, cx| {
                    root.fail_peer_cycle(
                        &cycle,
                        "Event poll outcome unknown; reconnect for fresh native recovery.".into(),
                        cx,
                    )
                });
                return;
            }
            let result = sdk.take_control().await;
            let _ = root.update_in(cx, |root, _, cx| {
                if !root.peers.current(&cycle) {
                    return;
                }
                root.peer_ui.attaching.remove(&cycle.peer);
                root.peer_ui.paused.remove(&cycle.peer);
                view.update(cx, |view, cx| view.publish(sdk, result, cx));
                cx.notify();
            });
        })
        .detach();
    }
}
