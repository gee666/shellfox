//! One event reader per window+peer. Pause and DRAIN the original captured
//! target set before attaching a view. Cancellation is only terminal retirement.
use super::*;
use crate::host_client::peer::Connection;
use shellfox_mobile::ManagerPollContext;
use shellfox_protocol::CancellationToken;
use std::sync::Arc;
use tokio::sync::oneshot;
#[cfg(test)]
#[path = "peer_poll_tests.rs"]
mod tests;
pub(super) struct PollSlot {
    cancel: CancellationToken,
    done: Option<oneshot::Receiver<()>>,
}
impl PollSlot {
    fn drain(&mut self) -> Option<oneshot::Receiver<()>> {
        self.done.take()
    }
}
impl Drop for PollSlot {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl Root {
    pub(super) fn fail_peer_cycle(
        &mut self,
        cycle: &source::PeerCycle,
        error: String,
        cx: &mut Context<Self>,
    ) {
        if !self.peers.current(cycle) {
            return;
        }
        self.retire_peer_views(&cycle.peer, cx);
        self.peers
            .phase(cycle, source::Phase::Reconnecting, Some(error));
        cx.notify();
    }
    pub(super) fn pause_peer_poll(
        &mut self,
        peer: &AuthenticatedPeerId,
    ) -> Option<oneshot::Receiver<()>> {
        self.peer_ui.paused.insert(peer.clone());
        self.peer_ui.polls.get_mut(peer).and_then(PollSlot::drain)
    }
    pub(crate) fn poll_peer_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing_window || self.peer_ui.frozen {
            return;
        }
        let recover: Vec<_> = self
            .peer_ui
            .recover
            .iter()
            .filter(|(peer, _)| !self.peer_ui.attaching.contains(peer))
            .cloned()
            .collect();
        for (peer, tab) in recover {
            if self.peer_ui.attaching.contains(&peer) {
                continue;
            }
            self.peer_ui.recover.remove(&(peer.clone(), tab));
            if let Some(view) = self.peer_ui.views.remove(&(peer.clone(), tab)) {
                view.update(cx, |view, _| view.retire());
            }
            if let Some(state) = self.peers.peers.get(&peer) {
                self.open_peer_terminal(
                    source::PeerCycle {
                        peer,
                        generation: state.cycle,
                    },
                    tab,
                    window,
                    cx,
                );
            }
        }
        let peers: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter(|(peer, state)| {
                state.phase == source::Phase::Connected
                    && !self.peer_ui.paused.contains(*peer)
                    && !self.peer_ui.polls.contains_key(*peer)
            })
            .map(|(peer, state)| (peer.clone(), state.cycle))
            .collect();
        for (peer, generation) in peers {
            let cycle = source::PeerCycle {
                peer: peer.clone(),
                generation,
            };
            let connection = match self
                .host
                .as_ref()
                .and_then(|host| host.peer_existing(&peer).ok())
            {
                Some(value) => value,
                None => continue,
            };
            let Some(witness) = self.peers.peers[&peer].origin.clone() else {
                continue;
            };
            let admission = match connection.admit_source_with(&witness) {
                Ok(value) => value,
                Err(error) => {
                    self.fail_peer_cycle(&cycle, error.to_string(), cx);
                    continue;
                }
            };
            let views: Vec<_> = self
                .peer_ui
                .views
                .iter()
                .filter(|((source, _), view)| source == &peer && view.read(cx).source == cycle)
                .filter_map(|(_, view)| view.read(cx).sdk.clone())
                .collect();
            let client = connection.client();
            let targets: Vec<_> = views.iter().map(Arc::as_ref).collect();
            let context = match ManagerPollContext::capture(&client, &targets) {
                Ok(value) => value,
                Err(error) => {
                    self.error = Some(format!("Peer poll capture: {error:?}"));
                    continue;
                }
            };
            let cancel = CancellationToken::new();
            let stop = cancel.clone();
            let (done, receive) = oneshot::channel();
            self.peer_ui.polls.insert(
                peer.clone(),
                PollSlot {
                    cancel,
                    done: Some(receive),
                },
            );
            cx.spawn_in(window,async move|root,cx|{
                let result={tokio::select!{biased;_=stop.cancelled()=>None,value=context.poll(&client)=>Some(value)}};
                // Normal pause drains this original poll through SDK application.
                // Only terminal retirement cancels; those views cannot resume.
                let _=root.update_in(cx,|root,window,cx|{
                    if !root.peers.current(&cycle){return;}
                    root.peer_ui.polls.remove(&peer);
                    let Some(result)=result else{return;};
                    if admission.check().is_err(){return;}
                    match result{
                        Ok(m::Reply::Success(response))=>root.peer_events(&cycle,&connection,response.events,window,cx),
                        Ok(m::Reply::Failure(error))=>{root.retire_peer_views(&peer,cx);root.peers.phase(&cycle,source::Phase::Rejected,Some(format!("{:?}",error.code)));},
                        Err(error)=>{root.retire_peer_views(&peer,cx);root.peers.phase(&cycle,source::Phase::Reconnecting,Some(format!("{error:?}")));},
                    }cx.notify();
                });
                let _=done.send(());
            }).detach();
        }
    }
    fn peer_events(
        &mut self,
        cycle: &source::PeerCycle,
        connection: &Arc<Connection>,
        events: Vec<m::HostEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        let mut recover = HashSet::new();
        for event in events {
            match event {
                m::HostEvent::Changed { .. } => changed = true,
                m::HostEvent::Terminal { event } => {
                    let tab = event.delivery().tab_id;
                    let Some(view) = self.peer_ui.views.get(&(cycle.peer.clone(), tab)).cloned()
                    else {
                        continue;
                    };
                    match event {
                        m::Event::Layout {
                            recovery_required: true,
                            ..
                        } => {
                            recover.insert(tab);
                        }
                        m::Event::SourceEnded { .. } | m::Event::ProcessExited { .. } => view
                            .update(cx, |view, cx| {
                                view.source_ended();
                                cx.notify();
                            }),
                        m::Event::Revoked {
                            reason:
                                m::RevokedReason::ControlLost | m::RevokedReason::ControlReleased,
                            ..
                        } => view.update(cx, |view, cx| {
                            view.input_state = None;
                            view.error = Some(
                                "Control withdrawn. Take control explicitly to resume input."
                                    .into(),
                            );
                            cx.notify();
                        }),
                        m::Event::Revoked { reason, .. } => view.update(cx, |view, cx| {
                            view.retire();
                            view.error = Some(format!("Authority withdrawn: {reason:?}"));
                            cx.notify();
                        }),
                        m::Event::Error { error, .. } => view.update(cx, |view, cx| {
                            view.error = Some(format!("Remote terminal: {:?}", error.code));
                            cx.notify();
                        }),
                        _ => {}
                    }
                }
            }
        }
        for tab in recover {
            // While close is cancelable, retain the original owner. The normal
            // unfrozen coordinator replaces it through native recovery later.
            self.peer_ui.recover.insert((cycle.peer.clone(), tab));
        }
        if changed {
            self.refresh_peer_catalog(cycle.clone(), connection.clone(), window, cx);
        }
    }
    fn refresh_peer_catalog(
        &mut self,
        cycle: source::PeerCycle,
        connection: Arc<Connection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(witness) = self
            .peers
            .peers
            .get(&cycle.peer)
            .and_then(|state| state.origin.clone())
        else {
            return;
        };
        let admission = match connection.admit_source_with(&witness) {
            Ok(value) => value,
            Err(_) => return,
        };
        cx.spawn_in(window, async move |root, cx| {
            let result = connection
                .execute_admitted(
                    &admission,
                    &m::GetSnapshot(shellfox_protocol::GetSnapshot {}),
                )
                .await;
            let _ = root.update_in(cx, |root, _, cx| {
                if !root.peers.current(&cycle) {
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        if root.peers.catalog(&cycle, snapshot).is_ok() {
                            for ((peer, tab), view) in &root.peer_ui.views {
                                if peer == &cycle.peer {
                                    let settled = root.peers.peers[peer]
                                        .snapshot
                                        .as_ref()
                                        .and_then(|snapshot| {
                                            crate::host_client::peer::settlement(snapshot, *tab)
                                        });
                                    view.update(cx, |view, _| view.hints.settled_at = settled);
                                }
                            }
                        }
                    }
                    Err(error) => root.notify_machine_error(&error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
