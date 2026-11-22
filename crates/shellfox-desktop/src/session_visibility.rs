use crate::root::{
    Root,
    source::{PeerCycle, Phase},
};
use gpui::{Bounds, Context, IntoElement, Pixels, Window, canvas, prelude::*, px};
use shellfox_core::SessionId;
use shellfox_protocol::{AuthenticatedPeerId, SetVisibleSessions};
use shellfox_service::native::NativeAction;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone)]
pub(crate) enum PollingTarget {
    Local(SessionId),
    Peer(AuthenticatedPeerId, SessionId),
}

#[derive(Default)]
pub(crate) struct SessionVisibility {
    pub(crate) trace: bool,
    painted: Rc<RefCell<Vec<PollingTarget>>>,
    local: Lane,
    peers: HashMap<AuthenticatedPeerId, (u64, Lane)>,
}

#[derive(Default)]
struct Lane {
    reported: Option<Vec<SessionId>>,
    sending: bool,
}

impl Lane {
    fn admit(&mut self, mut ids: Vec<SessionId>) -> Option<SetVisibleSessions> {
        ids.sort_unstable();
        ids.dedup();
        if self.sending || self.reported.as_ref() == Some(&ids) {
            return None;
        }
        self.reported = Some(ids.clone());
        self.sending = true;
        Some(SetVisibleSessions { session_ids: ids })
    }
}

impl SessionVisibility {
    pub(crate) fn begin_frame(&self) {
        self.painted.borrow_mut().clear();
    }

    pub(crate) fn marker(&self, target: PollingTarget) -> impl IntoElement + use<> {
        let painted = self.painted.clone();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                if intersects(bounds, window.content_mask().bounds) {
                    painted.borrow_mut().push(target);
                }
            },
        )
        .absolute()
        .inset_0()
    }
}

fn intersects(bounds: Bounds<Pixels>, clip: Bounds<Pixels>) -> bool {
    let intersection = bounds.intersect(&clip);
    intersection.size.width > px(0.) && intersection.size.height > px(0.)
}

impl Root {
    pub(crate) fn poll_session_visibility(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.closing_window {
            return;
        }
        let mut local = Vec::new();
        let mut peers: HashMap<AuthenticatedPeerId, Vec<SessionId>> = HashMap::new();
        if window_visible(window) {
            for target in self.sidebar_ui.visibility.painted.borrow().iter() {
                match target {
                    PollingTarget::Local(id) => local.push(*id),
                    PollingTarget::Peer(peer, id) => {
                        peers.entry(peer.clone()).or_default().push(*id)
                    }
                }
            }
            if !self.settings
                && !self
                    .files
                    .as_ref()
                    .is_some_and(|files| files.visible && files.maximized)
            {
                if let Some(session) = self.selected_session()
                    && self.selected_tab().is_some_and(|id| {
                        self.views.get(&id).is_some_and(|view| {
                            view.read(cx)
                                .runtime_phase()
                                .is_some_and(|phase| phase != shellfox_runtime::Phase::Starting)
                        })
                    })
                {
                    local.push(session);
                } else if let Some((peer, state)) = self.peers.selected_peer()
                    && state.phase == Phase::Connected
                    && let Some(session) = state.selected_session
                    && let Some(tab) = state.selected_tab
                    && let Some(entry) = self.peer_session(
                        &PeerCycle {
                            peer: peer.clone(),
                            generation: state.cycle,
                        },
                        session,
                    )
                    && entry.tabs.iter().any(|item| {
                        item.id == tab
                            && !matches!(
                                item.facts.lifecycle.known(),
                                Some(
                                    shellfox_protocol::TabLifecycle::Launching
                                        | shellfox_protocol::TabLifecycle::Closed
                                )
                            )
                    })
                    && let Ok(id) = session.to_string().parse()
                {
                    peers.entry(peer.clone()).or_default().push(id);
                }
            }
        }
        if let Some(host) = self.host.clone()
            && let Some(report) = self.sidebar_ui.visibility.local.admit(local)
        {
            let trace = self.sidebar_ui.visibility.trace.then(|| report.session_ids.clone());
            cx.spawn(async move |root, cx| {
                let result = host.execute(NativeAction::SetVisibleSessions(report)).await;
                if let (Some(ids), Ok(shellfox_service::native::NativeResponse::Applied)) = (trace, result) {
                    if let Ok(json) = serde_json::to_string(&ids) { eprintln!("UI visible local sessions: {json}"); }
                }
                let _ = root.update(cx, |root, _| {
                    if root
                        .host
                        .as_ref()
                        .is_some_and(|client| client.same_window_client(&host))
                    {
                        root.sidebar_ui.visibility.local.sending = false;
                    }
                });
            })
            .detach();
        }
        self.sidebar_ui
            .visibility
            .peers
            .retain(|peer, (generation, _)| {
                self.peers.peers.get(peer).is_some_and(|state| {
                    state.cycle == *generation && state.phase == Phase::Connected
                })
            });
        let connected: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter_map(|(peer, state)| {
                (state.phase == Phase::Connected).then_some(PeerCycle {
                    peer: peer.clone(),
                    generation: state.cycle,
                })
            })
            .collect();
        for cycle in connected {
            let Ok(api) = self.peer_api(&cycle) else {
                continue;
            };
            let (_, lane) = self
                .sidebar_ui
                .visibility
                .peers
                .entry(cycle.peer.clone())
                .or_insert_with(|| (cycle.generation, Lane::default()));
            let Some(report) = lane.admit(peers.remove(&cycle.peer).unwrap_or_default()) else {
                continue;
            };
            cx.spawn(async move |root, cx| {
                let _ = api
                    .call(report, |_| ())
                    .await;
                let _ = root.update(cx, |root, _| {
                    if let Some((generation, lane)) =
                        root.sidebar_ui.visibility.peers.get_mut(&cycle.peer)
                        && *generation == cycle.generation
                    {
                        lane.sending = false;
                    }
                });
            })
            .detach();
        }
    }
}

#[cfg(windows)]
fn window_visible(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{IsIconic, IsWindowVisible},
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    // A borrowed GPUI Window owns this HWND; these calls only read its state.
    unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() }
}

#[cfg(target_os = "macos")]
fn window_visible(window: &Window) -> bool {
    crate::macos_window::with_window(window, |window| {
        window.isVisible() && !window.isMiniaturized()
    })
    .unwrap_or(false)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn window_visible(_window: &Window) -> bool {
    // GPUI's Linux backend doesn't expose the compositor's minimize state.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    #[test]
    fn edge_touching_prefetched_rows_are_not_polling_targets() {
        let row = Bounds::new(point(px(0.), px(36.)), size(px(220.), px(36.)));
        assert!(!intersects(
            row,
            Bounds::new(point(px(0.), px(0.)), size(px(220.), px(36.)))
        ));
        assert!(intersects(
            row,
            Bounds::new(point(px(0.), px(0.)), size(px(220.), px(37.)))
        ));
    }

    #[test]
    fn reports_only_latest_distinct_set_after_previous_send_completes() {
        let a = SessionId::new();
        let b = SessionId::new();
        let mut lane = Lane::default();
        assert_eq!(lane.admit(vec![a, a]).unwrap().session_ids, [a]);
        assert!(lane.admit(vec![b]).is_none());
        lane.sending = false;
        assert_eq!(lane.admit(vec![]).unwrap().session_ids, []);
        lane.sending = false;
        assert!(lane.admit(vec![]).is_none());
    }
}
