use super::*;
use crate::terminal::runtime::physical;
use shellfox_ghostty_vt::key::KeyEvent;
use shellfox_ghostty_vt::{MouseAction, MouseButton, MouseEvent, MouseGeometry};

const BUTTONS: [MouseButton; 5] = [
    MouseButton::Left,
    MouseButton::Middle,
    MouseButton::Right,
    MouseButton::Back,
    MouseButton::Forward,
];

#[derive(Default)]
pub(super) struct Pointer {
    buttons: u8,
    last: Option<(MouseEvent, MouseGeometry)>,
}

impl Peer {
    fn queue_keyboard_input(&mut self, bytes: Vec<u8>) -> anyhow::Result<bool> {
        if bytes.is_empty() {
            return Ok(false);
        }
        self.queue(bytes, true)?;
        // User input follows the active buffer and clears the selection,
        // including keys the shell does not echo.
        self.terminal.view_select(&mut self.presentation, None)?;
        self.terminal.view_follow_tail(&mut self.presentation)?;
        Ok(true)
    }
}

impl TerminalView {
    pub(in crate::terminal) fn peer_key(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        let Some(peer) = self.peer_mut().filter(|peer| peer.can_write()) else {
            return Ok(());
        };
        if !peer.held_keys.admit(&key) {
            return Ok(());
        }
        let data = peer.terminal.encode_key_event(&key)?;
        if peer.queue_keyboard_input(data)? {
            self.refresh_peer_screen();
        }
        Ok(())
    }

    pub(in crate::terminal) fn peer_key_release(
        &self,
        key: physical::KeyIdentity,
    ) -> Option<KeyEvent> {
        match &self.backend {
            Backend::Peer(peer) => peer.held_keys.release(key.0, key.1),
            _ => None,
        }
    }

    /// Focus loss: the key-ups will go elsewhere, so release held keys now.
    pub(in crate::terminal) fn release_peer_keys(&mut self) {
        let releases = match self.peer_mut() {
            Some(peer) => peer.held_keys.release_all(),
            None => return,
        };
        for release in releases {
            let Some(peer) = self.peer_mut().filter(|peer| peer.can_write()) else {
                return;
            };
            let sent = peer
                .terminal
                .encode_key_event(&release)
                .map_err(anyhow::Error::from)
                .and_then(|data| peer.queue_keyboard_input(data));
            if let Err(error) = sent {
                self.reject(error);
            }
        }
    }

    pub(in crate::terminal) fn peer_paste(&mut self, text: &str) -> anyhow::Result<()> {
        let Some(peer) = self.peer_mut().filter(|peer| peer.can_write()) else {
            return Ok(());
        };
        let data = peer.terminal.encode_paste(text)?;
        if peer.queue_keyboard_input(data)? {
            self.refresh_peer_screen();
        }
        Ok(())
    }

    pub(in crate::terminal) fn peer_can_write(&self) -> bool {
        matches!(&self.backend, Backend::Peer(peer) if peer.can_write())
    }

    pub(in crate::terminal) fn peer_buttons(&self) -> u8 {
        match &self.backend {
            Backend::Peer(peer) => peer.pointer.buttons,
            _ => 0,
        }
    }

    pub(in crate::terminal) fn peer_mouse(
        &mut self,
        mut event: MouseEvent,
        geometry: MouseGeometry,
    ) -> bool {
        let Some(peer) = self.peer_mut().filter(|peer| peer.can_write()) else {
            return false;
        };
        let bit = BUTTONS
            .iter()
            .position(|button| Some(*button) == event.button)
            .map_or(0, |i| 1 << i);
        let buttons = match event.action {
            MouseAction::Press => peer.pointer.buttons | bit,
            MouseAction::Release => peer.pointer.buttons & !bit,
            MouseAction::Motion => peer.pointer.buttons,
        };
        event.any_button_pressed = buttons != 0;
        let result = peer
            .terminal
            .encode_mouse(&event, geometry)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| peer.queue(bytes, event.action != MouseAction::Motion));
        match result {
            Ok(()) => {
                peer.pointer.buttons = buttons;
                peer.pointer.last = Some((event, geometry));
                true
            }
            Err(error) => {
                self.reject(error);
                false
            }
        }
    }

    pub(in crate::terminal) fn cancel_peer_pointer(&mut self) {
        let Some(peer) = self.peer_mut() else {
            return;
        };
        let buttons = peer.pointer.buttons;
        let Some((mut event, geometry)) = peer.pointer.last else {
            return;
        };
        event.action = MouseAction::Release;
        for (i, button) in BUTTONS.into_iter().enumerate() {
            if buttons & (1 << i) != 0 {
                event.button = Some(button);
                self.peer_mouse(event, geometry);
            }
        }
        if let Some(peer) = self.peer_mut() {
            peer.pointer = Pointer::default();
        }
    }

    pub(in crate::terminal) fn report_peer_focus(&mut self, focused: bool) {
        let Some(peer) = self.peer_mut().filter(|peer| peer.can_write()) else {
            return;
        };
        let result = peer
            .terminal
            .encode_focus(focused)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| {
                let next = (!bytes.is_empty()).then_some(focused);
                if peer.reported_focus != next {
                    peer.queue(bytes, false)?;
                    peer.reported_focus = next;
                }
                Ok(())
            });
        if let Err(error) = result {
            self.reject(error);
        }
    }
}
