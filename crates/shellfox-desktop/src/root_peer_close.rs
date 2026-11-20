//! Window-close integration. A cancelable freeze never retires an SDK view.
use super::Root;
use gpui::{App, Context};
impl Root {
    pub(crate) fn peer_pending_operations(&self, _cx: &App) -> bool {
        if !self.peer_ui.pending.is_empty() || self.peers.peers.values().any(|peer| peer.pending) { return true; }
        #[cfg(feature = "experimental-shellfox-native")]
        return !self.peer_ui.attaching.is_empty()
            || !self.peer_ui.recover.is_empty()
            || self.peer_ui.views.values().any(|view| view.read(_cx).pending_operations())
            || (self.peer_ui.frozen && !self.peer_ui.polls_drained());
        #[cfg(not(feature = "experimental-shellfox-native"))]
        { self.peer_ui.v1_views.values().any(|view| view.read(_cx).peer_pending()) }
    }
    /// False means busy or still draining an ORIGINAL in-flight poll. The
    /// staged freeze stays set while draining; cancel explicitly with false.
    pub(crate) fn freeze_peer_inputs(&mut self, frozen: bool, cx: &mut Context<Self>) -> bool {
        if !frozen {
            if self.peer_ui.withdrawn {
                return false;
            }
            self.peer_ui.frozen = false;
            #[cfg(feature = "experimental-shellfox-native")]
            for view in self.peer_ui.views.values() {
                view.update(cx, |view, _| view.freeze_inputs(false));
            }
            #[cfg(not(feature = "experimental-shellfox-native"))]
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
        #[cfg(feature = "experimental-shellfox-native")]
        for view in self.peer_ui.views.values() {
            if !view.update(cx, |view, _| view.freeze_inputs(true)) {
                return false;
            }
        }
        #[cfg(not(feature = "experimental-shellfox-native"))]
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
            #[cfg(not(feature = "experimental-shellfox-native"))]
            self.retire_v1_terminals(&peer);
            if let Err(error) = self.peers.retire(&peer) {
                self.error = Some(error.to_string());
            }
        }
        cx.notify();
    }
}
