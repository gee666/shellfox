//! TTY events are logical, not PC scancodes. Never invent letter positions or
//! consumed/unshifted/side metadata the terminal did not report.
use anyhow::{Result, bail, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use shellfox_protocol::{Validate, manager_v2 as m};
pub(crate) fn key(event: KeyEvent) -> Result<m::KeyEventV2> {
    use m::PhysicalKey as P;
    ensure!(
        !event
            .modifiers
            .intersects(KeyModifiers::HYPER | KeyModifiers::META),
        "TTY reported modifiers outside the negotiated native key vocabulary."
    );
    let keypad = event.state.contains(KeyEventState::KEYPAD);
    let physical = match event.code {
        KeyCode::Backspace => {
            if keypad {
                P::NumpadBackspace
            } else {
                P::Backspace
            }
        }
        KeyCode::Enter => {
            if keypad {
                P::NumpadEnter
            } else {
                P::Enter
            }
        }
        KeyCode::Left => {
            if keypad {
                P::NumpadLeft
            } else {
                P::ArrowLeft
            }
        }
        KeyCode::Right => {
            if keypad {
                P::NumpadRight
            } else {
                P::ArrowRight
            }
        }
        KeyCode::Up => {
            if keypad {
                P::NumpadUp
            } else {
                P::ArrowUp
            }
        }
        KeyCode::Down => {
            if keypad {
                P::NumpadDown
            } else {
                P::ArrowDown
            }
        }
        KeyCode::Home => {
            if keypad {
                P::NumpadHome
            } else {
                P::Home
            }
        }
        KeyCode::End => {
            if keypad {
                P::NumpadEnd
            } else {
                P::End
            }
        }
        KeyCode::PageUp => {
            if keypad {
                P::NumpadPageUp
            } else {
                P::PageUp
            }
        }
        KeyCode::PageDown => {
            if keypad {
                P::NumpadPageDown
            } else {
                P::PageDown
            }
        }
        KeyCode::Delete => {
            if keypad {
                P::NumpadDelete
            } else {
                P::Delete
            }
        }
        KeyCode::Insert => {
            if keypad {
                P::NumpadInsert
            } else {
                P::Insert
            }
        }
        KeyCode::Tab | KeyCode::BackTab => P::Tab,
        KeyCode::Esc => P::Escape,
        KeyCode::CapsLock => P::CapsLock,
        KeyCode::ScrollLock => P::ScrollLock,
        KeyCode::NumLock => P::NumLock,
        KeyCode::PrintScreen => P::PrintScreen,
        KeyCode::Pause => P::Pause,
        KeyCode::Menu => P::ContextMenu,
        KeyCode::F(number) => {
            let symbol = format!("F{number}");
            P::ALL
                .iter()
                .copied()
                .find(|key| key.symbol() == symbol)
                .ok_or_else(|| anyhow::anyhow!("Unsupported TTY function key {number}"))?
        }
        KeyCode::Char(ch) if keypad => match ch {
            '0' => P::Numpad0,
            '1' => P::Numpad1,
            '2' => P::Numpad2,
            '3' => P::Numpad3,
            '4' => P::Numpad4,
            '5' => P::Numpad5,
            '6' => P::Numpad6,
            '7' => P::Numpad7,
            '8' => P::Numpad8,
            '9' => P::Numpad9,
            '+' => P::NumpadAdd,
            '-' => P::NumpadSubtract,
            '*' => P::NumpadMultiply,
            '/' => P::NumpadDivide,
            '.' => P::NumpadDecimal,
            ',' => P::NumpadComma,
            '=' => P::NumpadEqual,
            _ => bail!("TTY reported an unsupported keypad key."),
        },
        KeyCode::Char(_) => P::Unidentified,
        other => bail!("TTY key has no native physical representation: {other:?}"),
    };
    let mut bits = 0;
    for (flag, bit) in [
        (KeyModifiers::SHIFT, 1),
        (KeyModifiers::CONTROL, 2),
        (KeyModifiers::ALT, 4),
        (KeyModifiers::SUPER, 8),
    ] {
        if event.modifiers.contains(flag) {
            bits |= bit;
        }
    }
    if event.code == KeyCode::BackTab {
        bits |= 1;
    }
    if event.state.contains(KeyEventState::CAPS_LOCK) {
        bits |= 16;
    }
    if event.state.contains(KeyEventState::NUM_LOCK) {
        bits |= 32;
    }
    let text = match event.code {
        KeyCode::Char(ch) => ch.to_string(),
        _ => String::new(),
    };
    let event = m::KeyEventV2 {
        physical,
        action: match event.kind {
            KeyEventKind::Press => m::KeyActionV2::Press,
            KeyEventKind::Repeat => m::KeyActionV2::Repeat,
            KeyEventKind::Release => m::KeyActionV2::Release,
        },
        modifiers: m::ModifiersV2::try_from_bits(bits)?,
        consumed_modifiers: m::ModifiersV2::NONE,
        text,
        unshifted_codepoint: None,
        composing: false,
    };
    event.validate()?;
    Ok(event)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tty_logical_letters_do_not_claim_pc_positions() {
        let event = key(KeyEvent::new(KeyCode::Char('é'), KeyModifiers::ALT)).unwrap();
        assert_eq!(event.physical, m::PhysicalKey::Unidentified);
        assert_eq!(event.text, "é");
        assert!(event.unshifted_codepoint.is_none());
    }
    #[test]
    fn reported_keypad_lock_and_repeat_survive() {
        let event = KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Repeat,
            state: KeyEventState::KEYPAD | KeyEventState::NUM_LOCK,
        };
        let event = key(event).unwrap();
        assert_eq!(event.physical, m::PhysicalKey::NumpadEnter);
        assert_eq!(event.modifiers.bits(), 34);
        assert_eq!(event.action, m::KeyActionV2::Repeat);
    }
}
