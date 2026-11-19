use super::*;
use shellfox_ghostty_vt::KeyEvent;

impl PeerTerminalView {
    pub(super) fn send_xterm_key(&mut self, event: KeyEvent, cx: &mut Context<Self>) {
        let Ok(bytes) = self.owner.encode_xterm_key(&event) else {
            return;
        };
        if self.admit_key_bytes(&bytes, true, cx) {
            let _ = self.owner.set_selection(None);
            let _ = self.owner.scroll(i32::MAX);
        }
    }

    pub(super) fn report_xterm_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.ready || self.frozen {
            self.reported_focus = None;
            return;
        }
        let focused = self.visible && window.is_window_active() && self.focus.is_focused(window);
        let Ok(bytes) = self.owner.encode_xterm_focus(focused) else {
            return;
        };
        let next = bytes.map(|_| focused);
        if self.reported_focus == next {
            return;
        }
        if bytes.is_none_or(|bytes| self.admit_key_bytes(bytes, false, cx)) {
            self.reported_focus = next;
        }
    }

    fn admit_key_bytes(&mut self, bytes: &[u8], user: bool, cx: &mut Context<Self>) -> bool {
        if bytes.is_empty() {
            return false;
        }
        let Ok(data) = shellfox_protocol::encode_chunk(bytes) else {
            return false;
        };
        let mut hints = self.hints.clone();
        if !user {
            hints.user_initiated = false;
            hints.activity_hint = false;
        }
        self.admit_with_hints(
            m::Input::Bytes {
                encoding: m::Encoding::Base64,
                data,
                origin: m::BytesOrigin::Key,
            },
            hints,
            cx,
        )
    }
}
