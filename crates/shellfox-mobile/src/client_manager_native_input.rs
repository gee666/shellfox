//! Native UI input projection. Host KeyV2/paste/mouse encoding stays authoritative.
use super::*;
use crate::client::runtime::*;
use shellfox_protocol::Validate;
pub(super) fn project(
    mobile: &Mobile,
    engine: u64,
    json: &[u8],
    hint: bool,
) -> Result<Option<(m::Input, bool)>> {
    let value: serde_json::Value = serde_json::from_slice(json).map_err(|_| INVALID)?;
    if value.get("kind").is_some() {
        let input: m::Input = serde_json::from_value(value).map_err(|_| INVALID)?;
        input.validate().map_err(|_| INVALID)?;
        let user = hint && initiated(&input);
        return Ok(Some((input, user)));
    }
    let input: crate::input::Input = serde_json::from_value(value).map_err(|_| INVALID)?;
    let user = hint && input.user_initiated();
    use crate::input::{
        Action as A, Button as B, Input as I, PhysicalKey as K, PointerAction as P,
    };
    let input = match input {
        I::Preedit { text } => {
            if text.chars().any(char::is_control) {
                return Err(INVALID);
            }
            return Ok(None);
        }
        I::Paste { text } => m::Input::Paste { text },
        // IME committed text has no physical key. This is an explicit logical
        // commit, never fallback for an unrecognized hardware key.
        I::Commit { text } => m::Input::Key {
            event: key(
                m::PhysicalKey::Unidentified,
                m::KeyActionV2::Press,
                0,
                0,
                text,
                0,
            )?,
        },
        I::Key {
            key: physical,
            action,
            modifiers,
            consumed_modifiers,
            text,
            unshifted_codepoint,
        } => {
            let physical = match physical {
                K::Unidentified => m::PhysicalKey::Unidentified,
                K::Character(_) => return Err(INVALID),
                K::Enter => m::PhysicalKey::Enter,
                K::Tab => m::PhysicalKey::Tab,
                K::Backspace => m::PhysicalKey::Backspace,
                K::Escape => m::PhysicalKey::Escape,
                K::ArrowUp => m::PhysicalKey::ArrowUp,
                K::ArrowDown => m::PhysicalKey::ArrowDown,
                K::ArrowLeft => m::PhysicalKey::ArrowLeft,
                K::ArrowRight => m::PhysicalKey::ArrowRight,
                K::Home => m::PhysicalKey::Home,
                K::End => m::PhysicalKey::End,
                K::PageUp => m::PhysicalKey::PageUp,
                K::PageDown => m::PhysicalKey::PageDown,
                K::Insert => m::PhysicalKey::Insert,
                K::Delete => m::PhysicalKey::Delete,
                K::Function(n) => {
                    if !(1..=25).contains(&n) {
                        return Err(INVALID);
                    }
                    let symbol = format!("F{n}");
                    m::PhysicalKey::ALL
                        .into_iter()
                        .find(|p| p.symbol() == symbol)
                        .ok_or(INVALID)?
                }
            };
            let action = match action {
                A::Press => m::KeyActionV2::Press,
                A::Repeat => m::KeyActionV2::Repeat,
                A::Release => m::KeyActionV2::Release,
            };
            m::Input::Key {
                event: key(
                    physical,
                    action,
                    modifiers,
                    consumed_modifiers,
                    text.unwrap_or_default(),
                    unshifted_codepoint,
                )?,
            }
        }
        I::Mouse {
            action,
            button,
            modifiers,
            x,
            y,
            cell_width,
            cell_height,
            any_button_pressed,
        } => {
            if modifiers & !15 != 0 {
                return Err(INVALID);
            }
            let event = m::MouseEvent {
                action: match action {
                    P::Press => m::MouseAction::Press,
                    P::Release => m::MouseAction::Release,
                    P::Motion => m::MouseAction::Motion,
                },
                button: button.map(|b| match b {
                    B::Left => m::MouseButton::Left,
                    B::Middle => m::MouseButton::Middle,
                    B::Right => m::MouseButton::Right,
                    B::WheelUp => m::MouseButton::WheelUp,
                    B::WheelDown => m::MouseButton::WheelDown,
                    B::WheelLeft => m::MouseButton::WheelLeft,
                    B::WheelRight => m::MouseButton::WheelRight,
                    B::Back => m::MouseButton::Back,
                    B::Forward => m::MouseButton::Forward,
                }),
                modifiers: m::Modifiers {
                    shift: modifiers & 1 != 0,
                    ctrl: modifiers & 2 != 0,
                    alt: modifiers & 4 != 0,
                    super_key: modifiers & 8 != 0,
                },
                x_px: f64::from(x),
                y_px: f64::from(y),
                any_button_pressed,
            };
            let b = super::bindings::by_engine(mobile, engine)?;
            let context = b.origin()?;
            let geometry = context
                .witness
                .commit_if_current(|| -> Result<m::GeometryStamp> {
                    let state = b.state.lock().map_err(|_| PANIC)?;
                    context.check(&state).map_err(|_| STALE)?;
                    let r = state.cursor.as_ref().ok_or(STALE)?.recovery();
                    Ok(m::GeometryStamp {
                        recovery_id: r.recovery_id(),
                        layout_revision: r.layout_revision(),
                        cols: r.cols(),
                        rows: r.rows(),
                        cell_width_px: cell_width,
                        cell_height_px: cell_height,
                    })
                })
                .map_err(super::operations::client_error)??;
            m::Input::Mouse { event, geometry }
        }
    };
    input.validate().map_err(|_| INVALID)?;
    Ok(Some((input, user)))
}
fn key(
    physical: m::PhysicalKey,
    action: m::KeyActionV2,
    modifiers: u8,
    consumed: u8,
    text: String,
    scalar: u32,
) -> Result<m::KeyEventV2> {
    if modifiers & !15 != 0 || consumed & !15 != 0 {
        return Err(INVALID);
    }
    let event = m::KeyEventV2 {
        physical,
        action,
        modifiers: m::ModifiersV2::try_from_bits(u32::from(modifiers)).map_err(|_| INVALID)?,
        consumed_modifiers: m::ModifiersV2::try_from_bits(u32::from(consumed))
            .map_err(|_| INVALID)?,
        text,
        unshifted_codepoint: if scalar == 0 {
            None
        } else {
            Some(m::UnshiftedScalar::try_from(scalar).map_err(|_| INVALID)?)
        },
        composing: false,
    };
    event.validate().map_err(|_| INVALID)?;
    Ok(event)
}
fn initiated(input: &m::Input) -> bool {
    match input {
        m::Input::Key { event } => event.action != m::KeyActionV2::Release,
        m::Input::Mouse { event, .. } => {
            !matches!(event.action, m::MouseAction::Motion) || event.any_button_pressed
        }
        m::Input::Paste { text } => !text.is_empty(),
        m::Input::Bytes { .. } => true,
    }
}
