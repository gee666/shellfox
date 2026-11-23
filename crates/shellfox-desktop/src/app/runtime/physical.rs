use shellfox_ghostty_vt::key::{KeyAction, KeyEvent, PhysicalKey};
use shellfox_ghostty_vt::{MouseAction, MouseButton, MouseEvent, MouseGeometry};
use shellfox_runtime::{Command, Lease};

pub(super) const MAX_HELD: usize = 64;
#[derive(Default)]
pub(super) struct Physical {
    keys: Vec<(Lease, KeyEvent)>,
    buttons: Vec<(Lease, MouseEvent, MouseGeometry)>,
}

pub(crate) fn button_bit(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 4,
        MouseButton::Back => 8,
        MouseButton::Forward => 16,
        MouseButton::Ten => 32,
        MouseButton::Eleven => 64,
        _ => 0,
    }
}

/// Physical key plus base character; committed text has neither and is never held.
pub(crate) type KeyIdentity = (PhysicalKey, Option<char>);
fn identity(event: &KeyEvent) -> Option<KeyIdentity> {
    event.identity()
}

/// A repeat or release whose press never reached the terminal, e.g. the
/// key-up of an app shortcut. Sending it would leak the shortcut to kitty
/// apps that report releases.
fn orphan(held: bool, event: &KeyEvent) -> bool {
    !held && event.action != KeyAction::Press && identity(event).is_some()
}

impl Physical {
    pub fn len(&self) -> usize {
        self.keys.len() + self.buttons.len()
    }
    pub fn buttons(&self) -> u8 {
        self.buttons
            .iter()
            .filter_map(|(_, event, _)| event.button)
            .fold(0, |mask, button| mask | button_bit(button))
    }
    pub fn key_release(&self, key: KeyIdentity) -> Option<KeyEvent> {
        let held = self.keys.iter().find(|(_, event)| identity(event) == Some(key))?;
        Some(held.1.release())
    }
    fn key_held(&self, event: &KeyEvent) -> bool {
        self.keys.iter().any(|(_, key)| identity(key) == identity(event))
    }
    pub fn change(&self, command: &Command) -> i8 {
        match command {
            Command::Key { event, .. } if identity(event).is_some() => {
                let held = self.key_held(event);
                match event.action {
                    KeyAction::Release if held => -1,
                    KeyAction::Press if !held => 1,
                    _ => 0,
                }
            }
            Command::Mouse { event, .. }
                if event.button.is_some_and(|button| button_bit(button) != 0) =>
            {
                let held = self
                    .buttons
                    .iter()
                    .any(|(_, button, _)| button.button == event.button);
                match event.action {
                    MouseAction::Release if held => -1,
                    MouseAction::Press if !held => 1,
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
    /// Releases and repeats whose press was never sent are dropped.
    pub fn unmatched(&self, command: &Command) -> bool {
        match command {
            Command::Key { event, .. } => orphan(self.key_held(event), event),
            Command::Mouse { event, .. } => {
                event.action == MouseAction::Release && self.change(command) != -1
            }
            _ => false,
        }
    }
    pub fn accept(&mut self, command: &mut Command) {
        match command {
            Command::Key { lease, event } if identity(event).is_some() => {
                self.keys.retain(|(_, key)| identity(key) != identity(event));
                if event.action != KeyAction::Release {
                    self.keys.push((*lease, event.clone()));
                }
            }
            Command::Mouse {
                lease,
                event,
                geometry,
            } => {
                if event.action != MouseAction::Motion
                    && event.button.is_some_and(|button| button_bit(button) != 0)
                {
                    self.buttons
                        .retain(|(_, button, _)| button.button != event.button);
                    if event.action == MouseAction::Press {
                        self.buttons.push((*lease, *event, *geometry));
                    }
                }
                event.any_button_pressed = self.buttons() != 0;
            }
            _ => {}
        }
    }
    pub fn releases(&self) -> Vec<Command> {
        let mut result = Vec::with_capacity(self.len());
        for (lease, original) in &self.keys {
            result.push(Command::Key {
                lease: *lease,
                event: original.release(),
            });
        }
        for (lease, original, geometry) in &self.buttons {
            let mut event = *original;
            event.action = MouseAction::Release;
            result.push(Command::Mouse {
                lease: *lease,
                event,
                geometry: *geometry,
            });
        }
        result
    }
}
