//! Window-close integration. A cancelable freeze never retires an SDK view.
use crate::root::Root;
use gpui::{App, Context};
impl Root {
    pub(crate) fn peer_pending_operations(&self, _cx: &App) -> bool {
        if !self.peer_ui.pending.is_empty() || self.peers.peers.values().any(|peer| peer.pending) { return true; }
        self.peer_ui.v1_views.values().any(|view| view.read(_cx).peer_pending())
    }
    /// False means busy or still draining an ORIGINAL in-flight poll. The
    /// staged freeze stays set while draining; cancel explicitly with false.
    pub(crate) fn freeze_peer_inputs(&mut self, frozen: bool, cx: &mut Context<Self>) -> bool {
        if !frozen {
            if self.peer_ui.withdrawn {
                return false;
            }
            self.peer_ui.frozen = false;
            self.peer_ui.stream_wake.notify_waiters();
            for view in self.peer_ui.v1_views.values() {
                view.update(cx, |view, _| view.freeze_peer(false));
            }
            cx.notify();
            return true;
        }
        if !self.peer_ui.frozen && self.peer_pending_operations(cx) {
            return false;
        }
        self.peer_ui.frozen = true;
        self.stop_peer_pairing();
        for view in self.peer_ui.v1_views.values() {
            view.update(cx, |view, _| view.freeze_peer(true));
        }
        // Do not drop the PollSlot. The original captured context must drain;
        // an absent completion signal is handled as unknown by the coordinator.
        cx.notify();
        !self.peer_pending_operations(cx)
    }
    /// Call only after successful withdrawal, or withdrawal entered and failed.
    pub(crate) fn retire_all_peer_views(&mut self, cx: &mut Context<Self>) {
        self.peer_ui.frozen = true;
        self.peer_ui.withdrawn = true;
        self.stop_peer_pairing();
        let peers: Vec<_> = self.peers.peers.keys().cloned().collect();
        for peer in peers {
            self.retire_peer_views(&peer, cx);
            self.retire_v1_terminals(&peer);
            if let Err(error) = self.peers.retire(&peer) {
                self.error = Some(error.to_string());
            }
        }
        cx.notify();
    }
}
