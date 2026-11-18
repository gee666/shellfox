//! GPUI logical/native-text capture to full KeyV2, without the lossy v1 Key DTO.
use gpui::Keystroke;
use shellfox_protocol::manager_v2 as m;
pub(crate) fn physical(
    stroke: &Keystroke,
    action: m::KeyActionV2,
    composing: bool,
) -> Option<m::KeyEventV2> {
    use m::PhysicalKey as P;
    let physical = match stroke.key.as_str() {
        "up" => P::ArrowUp,
        "down" => P::ArrowDown,
        "left" => P::ArrowLeft,
        "right" => P::ArrowRight,
        name => P::ALL
            .iter()
            .copied()
            .find(|key| key.symbol().eq_ignore_ascii_case(name))
            .unwrap_or(P::Unidentified),
    };
    let modified = stroke.modifiers.control || stroke.modifiers.alt || stroke.modifiers.platform;
    // Printable press/repeat is committed through the native IME path ONCE.
    if matches!(physical, P::Unidentified | P::Space)
        && !modified
        && action != m::KeyActionV2::Release
    {
        return None;
    }
    let text = if physical == P::Unidentified {
        stroke
            .key_char
            .clone()
            .filter(|text| !text.chars().any(char::is_control))
            .or_else(|| (stroke.key.chars().count() == 1).then(|| stroke.key.clone()))
            .unwrap_or_default()
    } else {
        String::new()
    };
    if physical == P::Unidentified && text.is_empty() {
        return None;
    }
    let modifiers = u32::from(stroke.modifiers.shift)
        | u32::from(stroke.modifiers.control) << 1
        | u32::from(stroke.modifiers.alt) << 2
        | u32::from(stroke.modifiers.platform) << 3;
    Some(m::KeyEventV2 {
        physical,
        action,
        modifiers: m::ModifiersV2::try_from_bits(modifiers).ok()?,
        consumed_modifiers: m::ModifiersV2::NONE,
        text,
        unshifted_codepoint: None,
        composing,
    })
}
pub(crate) fn committed(text: &str) -> m::KeyEventV2 {
    m::KeyEventV2 {
        physical: m::PhysicalKey::Unidentified,
        action: m::KeyActionV2::Press,
        modifiers: m::ModifiersV2::NONE,
        consumed_modifiers: m::ModifiersV2::NONE,
        text: text.into(),
        unshifted_codepoint: None,
        composing: false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn printable_commit_and_named_key_identity_are_separate() {
        assert!(
            physical(
                &Keystroke::parse("a").unwrap(),
                m::KeyActionV2::Press,
                false
            )
            .is_none()
        );
        assert_eq!(committed("é").physical, m::PhysicalKey::Unidentified);
        let event = physical(
            &Keystroke::parse("shift-up").unwrap(),
            m::KeyActionV2::Release,
            false,
        )
        .unwrap();
        assert_eq!(event.physical, m::PhysicalKey::ArrowUp);
        assert_eq!(event.modifiers.bits(), 1);
        assert_eq!(event.action, m::KeyActionV2::Release);
    }
}
