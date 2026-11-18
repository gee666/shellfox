use super::*;
fn event() -> m::KeyEventV2 {
    m::KeyEventV2 {
        physical: m::PhysicalKey::NumpadEnter,
        action: m::KeyActionV2::Release,
        modifiers: m::ModifiersV2::NONE,
        consumed_modifiers: m::ModifiersV2::NONE,
        text: String::new(),
        unshifted_codepoint: None,
        composing: false,
    }
}
#[test]
fn every_wire_physical_id_reaches_the_same_compiled_native_symbol() {
    for physical in m::PhysicalKey::ALL {
        let mut wire = event();
        wire.physical = physical;
        let native = key_v2(wire).unwrap();
        assert_eq!(native.physical.code(), physical.code());
        assert_eq!(format!("{:?}", native.physical), physical.symbol());
    }
}
#[test]
fn all_known_mask_patterns_survive_without_release_coherence_inference() {
    for bits in 0..=0x03ff {
        let mut wire = event();
        wire.modifiers = m::ModifiersV2::try_from_bits(bits).unwrap();
        wire.consumed_modifiers = m::ModifiersV2::try_from_bits(bits ^ 0x03ff).unwrap();
        let native = key_v2(wire).unwrap();
        assert_eq!(u32::from(native.modifiers.bits()), bits);
        assert_eq!(u32::from(native.consumed_modifiers.bits()), bits ^ 0x03ff);
        assert_eq!(native.action, shellfox_runtime::KeyActionV2::Release);
    }
}
#[test]
fn exact_c1_unicode_scalar_and_composition_reach_owned_fifo_event() {
    let mut wire = event();
    wire.text = "\u{80}\u{9f}😀".into();
    wire.unshifted_codepoint = Some(m::UnshiftedScalar::new('\u{10ffff}').unwrap());
    wire.composing = true;
    let native = key_v2(wire).unwrap();
    assert_eq!(native.text, "\u{80}\u{9f}😀");
    assert_eq!(native.unshifted_codepoint, Some('\u{10ffff}'));
    assert!(native.composing);
    let command = shellfox_runtime::Command::KeyV2 {
        lease: shellfox_runtime::Lease {
            controller: 7,
            epoch: 9,
        },
        event: native,
    };
    assert!(matches!(
        command,
        shellfox_runtime::Command::KeyV2 {
            event: shellfox_runtime::OwnedKeyEventV2 {
                physical: shellfox_runtime::PhysicalKey::NumpadEnter,
                ..
            },
            ..
        }
    ));
}
#[test]
fn invalid_sys_fields_and_missing_required_fields_reject_before_conversion() {
    let value = serde_json::to_value(event()).unwrap();
    for (key, number) in [
        ("physical", 176),
        ("physical", 65536),
        ("modifiers", 0x0400),
        ("consumedModifiers", 0x10000),
        ("unshiftedCodepoint", 0),
        ("unshiftedCodepoint", 0xd800),
        ("unshiftedCodepoint", 0x110000),
    ] {
        let mut invalid = value.clone();
        invalid[key] = serde_json::json!(number);
        assert!(serde_json::from_value::<m::KeyEventV2>(invalid).is_err());
    }
    for key in [
        "physical",
        "action",
        "modifiers",
        "consumedModifiers",
        "text",
        "unshiftedCodepoint",
        "composing",
    ] {
        let mut invalid = value.clone();
        invalid.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<m::KeyEventV2>(invalid).is_err(),
            "{key}"
        );
    }
    let mut invalid = event();
    invalid.text = "\0".into();
    assert!(key_v2(invalid).is_err());
    let mut overflow = event();
    overflow.text = "x".repeat(m::MAX_INPUT_PAYLOAD_BYTES + 1);
    assert!(key_v2(overflow).is_err());
}
