use super::*;
use shellfox_ghostty_vt as vt;
#[cfg(test)]
#[path = "manager_input_map_tests.rs"]
mod tests;
pub(super) fn modifiers(value: m::Modifiers) -> vt::Modifiers {
    vt::Modifiers {
        shift: value.shift,
        control: value.ctrl,
        alt: value.alt,
        super_key: value.super_key,
    }
}
pub(super) fn key_v2(value: m::KeyEventV2) -> Result<shellfox_runtime::OwnedKeyEventV2, AppError> {
    value
        .validate()
        .map_err(|_| failure(ErrorCode::Validation, "Invalid KeyV2 fields."))?;
    let physical =
        shellfox_runtime::PhysicalKey::try_from(value.physical.code()).map_err(|_| {
            failure(
                ErrorCode::Unsupported,
                "Pinned native physical-key vocabulary differs from manager-v2.",
            )
        })?;
    let action = shellfox_runtime::KeyActionV2::try_from(value.action.code())
        .map_err(|_| failure(ErrorCode::Validation, "Invalid KeyV2 action."))?;
    let modifiers = shellfox_runtime::ModifiersV2::try_from_bits(u32::from(value.modifiers.bits()))
        .map_err(|_| failure(ErrorCode::Validation, "Invalid active KeyV2 mask."))?;
    let consumed_modifiers =
        shellfox_runtime::ModifiersV2::try_from_bits(u32::from(value.consumed_modifiers.bits()))
            .map_err(|_| failure(ErrorCode::Validation, "Invalid consumed KeyV2 mask."))?;
    let unshifted_codepoint = value.unshifted_char();
    Ok(shellfox_runtime::OwnedKeyEventV2 {
        physical,
        action,
        modifiers,
        consumed_modifiers,
        text: value.text,
        unshifted_codepoint,
        composing: value.composing,
    })
}
pub(super) fn mouse(value: m::MouseEvent) -> Result<vt::MouseEvent, AppError> {
    value.validate().map_err(|_| {
        failure(
            ErrorCode::Validation,
            "Invalid native mouse coordinates before conversion.",
        )
    })?;
    Ok(vt::MouseEvent {
        action: match value.action {
            m::MouseAction::Press => vt::MouseAction::Press,
            m::MouseAction::Release => vt::MouseAction::Release,
            m::MouseAction::Motion => vt::MouseAction::Motion,
        },
        button: value.button.map(|value| match value {
            m::MouseButton::Left => vt::MouseButton::Left,
            m::MouseButton::Middle => vt::MouseButton::Middle,
            m::MouseButton::Right => vt::MouseButton::Right,
            m::MouseButton::WheelUp => vt::MouseButton::WheelUp,
            m::MouseButton::WheelDown => vt::MouseButton::WheelDown,
            m::MouseButton::WheelLeft => vt::MouseButton::WheelLeft,
            m::MouseButton::WheelRight => vt::MouseButton::WheelRight,
            m::MouseButton::Back => vt::MouseButton::Back,
            m::MouseButton::Forward => vt::MouseButton::Forward,
            m::MouseButton::Ten => vt::MouseButton::Ten,
            m::MouseButton::Eleven => vt::MouseButton::Eleven,
        }),
        modifiers: modifiers(value.modifiers),
        x: value.x_px as f32,
        y: value.y_px as f32,
        any_button_pressed: value.any_button_pressed,
    })
}
