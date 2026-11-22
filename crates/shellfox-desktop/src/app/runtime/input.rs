use super::super::{Backend, TerminalView};
use super::Pending;
use gpui::{Context, KeyUpEvent, Window};
use shellfox_ghostty_vt::key::KeyEvent;
use shellfox_runtime::Command;

impl TerminalView {
    pub fn send_key(&mut self, event: KeyEvent) {
        if self.failed || self.closing {
            return;
        }
        if let Some(lease) = self.lease {
            self.submit(Command::Key { lease, event }, Pending::Applied);
        } else if matches!(self.backend, Backend::Attached(_)) {
            self.reject("Terminal control is not ready; input was not accepted");
        } else if let Err(error) = self.peer_key(event) {
            self.reject(error);
        }
    }

    pub(in crate::app) fn physical_key_up(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Only keys whose press reached the terminal are released there; the
        // key-up of an app shortcut never is.
        let key = crate::input::key_identity(&event.keystroke.key);
        if let Some(release) = self.outbox.key_release(key).or_else(|| self.peer_key_release(key)) {
            // Modifiers may have been released before this physical key.
            self.send_key(release);
            cx.stop_propagation();
        } else {
            self.key_up(event, window, cx);
        }
    }

    pub(in crate::app) fn cancel_pointer(&mut self, cx: &mut Context<Self>) {
        self.selection_drag.finish();
        self.link_press = None;
        self.links.file_press = None;
        self.links.menu_press = None;
        self.end_scrollbar_drag();
        self.cancel_peer_pointer();
        if self.failed || self.closing {
            return;
        }
        if let Err(error) = self.cancel_held_input(true) {
            self.reject(error);
        }
        self.flush_outbox();
        cx.notify();
    }

    pub(in crate::app) fn cancel_input(&mut self, cx: &mut Context<Self>) {
        self.cancel_peer_pointer();
        self.release_peer_keys();
        let was_dragging_scrollbar = self.end_scrollbar_drag();
        let had_file_press = self.links.file_press.take().is_some();
        let had_menu_press = self.links.menu_press.take().is_some();
        if !was_dragging_scrollbar && !had_file_press && !had_menu_press && !self.outbox.has_held()
            && !self.selection_drag.active()
            && self.link_press.is_none()
            && self.clipboard_keys == 0
        {
            return;
        }
        self.selection_drag.finish();
        self.link_press = None;
        self.clipboard_keys = 0;
        if self.failed || self.closing {
            return;
        }
        if let Err(error) = self.cancel_held_input(false) {
            self.reject(error);
        }
        self.flush_outbox();
        cx.notify();
    }
}
