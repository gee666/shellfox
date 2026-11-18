use super::*;

fn event() -> KeyV2<'static> {
    KeyV2 {
        physical: PhysicalKey::KeyA,
        action: KeyActionV2::Press,
        modifiers: ModifiersV2::NONE,
        consumed_modifiers: ModifiersV2::NONE,
        text: "a",
        unshifted_codepoint: Some('a'),
        composing: false,
    }
}

#[test]
fn closed_vocabulary_has_exactly_176_unique_numeric_ids() {
    let public_c = include_str!("../../tests/input_v2_public.c");
    for (code, key) in PhysicalKey::ALL.into_iter().enumerate() {
        let code = u32::try_from(code).unwrap();
        assert_eq!(key.code(), code);
        assert_eq!(PhysicalKey::try_from(code), Ok(key));
        // The enabled C probe compiles each of these named assertions against
        // the selected header. Match every Rust variant name and ID to it.
        let assertion = format!(" == {code}, \"{key:?}\");");
        assert!(public_c.lines().any(|line| line.ends_with(&assertion)));
    }
    assert_eq!(PhysicalKey::KeyF.code(), 25);
    assert_eq!(PhysicalKey::F25.code(), 145);
    assert_eq!(PhysicalKey::Paste.code(), 175);
    for code in [176, i32::MAX as u32, u32::MAX] {
        assert_eq!(
            PhysicalKey::try_from(code),
            Err(InvalidKeyV2::PhysicalKey(code))
        );
    }
    for (code, action) in [
        (0, KeyActionV2::Release),
        (1, KeyActionV2::Press),
        (2, KeyActionV2::Repeat),
    ] {
        assert_eq!(KeyActionV2::try_from(code), Ok(action));
    }
    for code in [3, i32::MAX as u32, u32::MAX] {
        assert_eq!(KeyActionV2::try_from(code), Err(InvalidKeyV2::Action(code)));
    }
}

#[test]
fn all_known_modifier_patterns_survive_all_actions_without_inferred_coherence() {
    for bits in 0..=0x3ff {
        let mods = ModifiersV2::try_from_bits(bits).unwrap();
        assert_eq!(u32::from(mods.bits()), bits);
        for action in [
            KeyActionV2::Press,
            KeyActionV2::Repeat,
            KeyActionV2::Release,
        ] {
            let key = KeyV2 {
                action,
                modifiers: mods,
                consumed_modifiers: ModifiersV2::try_from_bits(bits ^ 0x3ff).unwrap(),
                ..event()
            };
            key.validate().unwrap();
            assert_eq!(key.raw().mods, bits);
            assert_eq!(key.raw().consumed_mods, bits ^ 0x3ff);
        }
    }
    let flags = [
        ModifiersV2::SHIFT,
        ModifiersV2::CONTROL,
        ModifiersV2::ALT,
        ModifiersV2::SUPER,
        ModifiersV2::CAPS_LOCK,
        ModifiersV2::NUM_LOCK,
        ModifiersV2::SHIFT_SIDE,
        ModifiersV2::CONTROL_SIDE,
        ModifiersV2::ALT_SIDE,
        ModifiersV2::SUPER_SIDE,
    ];
    for (index, flag) in flags.into_iter().enumerate() {
        assert_eq!(u32::from(flag.bits()), 1 << index);
        assert!(!ModifiersV2::NONE.contains(flag));
        assert!((ModifiersV2::NONE | flag).contains(flag));
    }
    for bit in 10..32 {
        let bits = 1 << bit;
        assert_eq!(
            ModifiersV2::try_from_bits(bits),
            Err(InvalidKeyV2::ModifierBits(bits))
        );
    }
}

#[test]
fn raw_sys_fields_validate_before_narrowing() {
    let template = event();
    for code in [176, u32::MAX] {
        let key = Key {
            key: code,
            ..template.raw()
        };
        assert_eq!(validate_raw(&key), Err(InvalidKeyV2::PhysicalKey(code)));
    }
    for code in [3, u32::MAX] {
        assert_eq!(
            validate_raw(&Key {
                action: code,
                ..event().raw()
            }),
            Err(InvalidKeyV2::Action(code))
        );
    }
    for bit in 10..32 {
        let bits = 1 << bit;
        assert_eq!(
            validate_raw(&Key {
                mods: bits,
                ..event().raw()
            }),
            Err(InvalidKeyV2::ModifierBits(bits))
        );
        assert_eq!(
            validate_raw(&Key {
                consumed_mods: bits,
                ..event().raw()
            }),
            Err(InvalidKeyV2::ModifierBits(bits))
        );
    }
    for scalar in (0xd800..=0xdfff).chain([0x110000, 0x200061, u32::MAX]) {
        assert_eq!(
            validate_raw(&Key {
                unshifted: scalar,
                ..event().raw()
            }),
            Err(InvalidKeyV2::Scalar(scalar))
        );
    }
    for scalar in [0, 0x61, 0x1f600, 0x10ffff] {
        validate_raw(&Key {
            unshifted: scalar,
            ..event().raw()
        })
        .unwrap();
    }
}

#[test]
fn typed_scalar_distinguishes_absent_from_unrepresentable_nul() {
    KeyV2 {
        unshifted_codepoint: None,
        ..event()
    }
    .validate()
    .unwrap();
    assert_eq!(
        KeyV2 {
            unshifted_codepoint: Some('\0'),
            ..event()
        }
        .validate(),
        Err(InvalidKeyV2::Scalar(0))
    );
    for c in ['a', 'é', '😀', '\u{10ffff}'] {
        let key = KeyV2 {
            unshifted_codepoint: Some(c),
            ..event()
        };
        key.validate().unwrap();
        assert_eq!(key.raw().unshifted, u32::from(c));
    }
}

#[test]
fn text_policy_matches_public_guide_not_char_is_control() {
    for text in [
        "",
        "a",
        "é",
        "a\u{301}😀",
        "\u{80}\u{85}\u{9f}",
        "\u{f6ff}\u{f900}",
    ] {
        KeyV2 { text, ..event() }.validate().unwrap();
    }
    for code in (0..=0x1f).chain([0x7f]).chain(0xf700..=0xf8ff) {
        let c = char::from_u32(code).unwrap();
        let text = format!("ok{c}tail");
        assert_eq!(
            KeyV2 {
                text: &text,
                ..event()
            }
            .validate(),
            Err(InvalidKeyV2::TextCodepoint(c))
        );
    }
    let text = "x".repeat(LIMIT);
    KeyV2 {
        text: &text,
        ..event()
    }
    .validate()
    .unwrap();
    let oversize = text + "x";
    let error = KeyV2 {
        text: &oversize,
        ..event()
    }
    .validate()
    .unwrap_err();
    assert_eq!(error, InvalidKeyV2::TextLimit(LIMIT + 1));
    assert_eq!(error.native_code(), -6);
}

#[cfg(sf_input_v2_public_probe)]
#[test]
fn exhaustive_compiled_public_c_probe_and_real_encoder_comparison() {
    unsafe extern "C" {
        fn sf_input_v2_public_probe() -> i32;
    }
    // SAFETY: the opt-in probe is compiled with this exact adapter and pinned
    // public headers/library. It owns and frees every native test object.
    let line = unsafe { sf_input_v2_public_probe() };
    assert_eq!(
        line, 0,
        "public C input probe failed at C source line {line}"
    );
}
