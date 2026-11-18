use shellfox_ghostty_vt::{
    Key, KeyAction, KeyEvent, MouseAction, MouseButton, MouseEvent, MouseGeometry,
};
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

fn same_key(first: Key, second: Key) -> bool {
    let canonical = |key| match key {
        Key::Character(ch) => Key::Character(ch.to_ascii_lowercase()),
        key => key,
    };
    canonical(first) == canonical(second)
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
    pub fn key_release(&self, key: Key) -> Option<KeyEvent> {
        let mut event = self
            .keys
            .iter()
            .find(|(_, event)| same_key(event.key, key))?
            .1
            .clone();
        event.action = KeyAction::Release;
        event.text = None;
        Some(event)
    }
    pub fn change(&self, command: &Command) -> i8 {
        match command {
            Command::Key { event, .. } if event.key != Key::Unidentified => {
                let held = self
                    .keys
                    .iter()
                    .any(|(_, key)| same_key(key.key, event.key));
                match event.action {
                    KeyAction::Release if held => -1,
                    KeyAction::Press | KeyAction::Repeat if !held => 1,
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
    pub fn unmatched_release(&self, command: &Command) -> bool {
        let release = matches!(command, Command::Key { event, .. } if event.action == KeyAction::Release)
            || matches!(command, Command::Mouse { event, .. } if event.action == MouseAction::Release);
        release && self.change(command) != -1
    }
    pub fn accept(&mut self, command: &mut Command) {
        match command {
            Command::Key { lease, event } if event.key != Key::Unidentified => {
                self.keys.retain(|(_, key)| !same_key(key.key, event.key));
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
            let mut event = original.clone();
            event.action = KeyAction::Release;
            event.text = None;
            result.push(Command::Key {
                lease: *lease,
                event,
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
