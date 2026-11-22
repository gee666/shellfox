use super::*;

fn event() -> KeyEvent<'static> {
    KeyEvent {
        physical: PhysicalKey::KeyA,
        action: KeyAction::Press,
        modifiers: KeyModifiers::NONE,
        consumed_modifiers: KeyModifiers::NONE,
        text: "a",
        unshifted_codepoint: Some('a'),
        composing: false,
    }
}

#[test]
fn closed_vocabulary_has_exactly_176_unique_numeric_ids() {
    let public_c = include_str!("../../tests/key_public.c");
    for (code, key) in PhysicalKey::ALL.into_iter().enumerate() {
        let code = u32::try_from(code).unwrap();
        assert_eq!(key.code(), code);
        assert_eq!(PhysicalKey::try_from(code), Ok(key));
        // The opt-in C probe compiles these named assertions against the
        // selected header. Match every Rust variant name and ID to it.
        let assertion = format!(" == {code}, \"{key:?}\");");
        assert!(public_c.lines().any(|line| line.ends_with(&assertion)));
    }
    assert_eq!(PhysicalKey::KeyF.code(), 25);
    assert_eq!(PhysicalKey::F25.code(), 145);
    assert_eq!(PhysicalKey::Paste.code(), 175);
    for code in [176, i32::MAX as u32, u32::MAX] {
        assert_eq!(
            PhysicalKey::try_from(code),
            Err(InvalidKey::PhysicalKey(code))
        );
    }
}

#[test]
fn all_known_modifier_patterns_survive() {
    for bits in 0..=0x3ff {
        let mods = KeyModifiers::try_from_bits(bits).unwrap();
        assert_eq!(u32::from(mods), bits);
    }
    let flags = [
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::CAPS_LOCK,
        KeyModifiers::NUM_LOCK,
        KeyModifiers::SHIFT_SIDE,
        KeyModifiers::CONTROL_SIDE,
        KeyModifiers::ALT_SIDE,
        KeyModifiers::SUPER_SIDE,
    ];
    for (index, flag) in flags.into_iter().enumerate() {
        assert_eq!(u32::from(flag.bits()), 1 << index);
        assert!(!KeyModifiers::NONE.contains(flag));
        assert!((KeyModifiers::NONE | flag).contains(flag));
        let mut mods = KeyModifiers::NONE;
        mods.set(flag, true);
        assert!(mods.contains(flag));
        mods.set(flag, false);
        assert!(mods.is_empty());
    }
    for bit in 10..32 {
        let bits = 1 << bit;
        assert_eq!(
            KeyModifiers::try_from_bits(bits),
            Err(InvalidKey::ModifierBits(bits))
        );
    }
}

#[test]
fn serde_uses_validated_numeric_codes() {
    let json = |value: &str| serde_json::from_str::<PhysicalKey>(value);
    assert_eq!(json("20").unwrap(), PhysicalKey::KeyA);
    assert!(json("176").is_err());
    assert_eq!(serde_json::to_string(&PhysicalKey::Enter).unwrap(), "58");
    assert!(serde_json::from_str::<KeyModifiers>("1024").is_err());
    assert_eq!(
        serde_json::from_str::<KeyAction>("\"repeat\"").unwrap(),
        KeyAction::Repeat
    );
}

#[test]
fn typed_scalar_distinguishes_absent_from_unrepresentable_nul() {
    KeyEvent {
        unshifted_codepoint: None,
        ..event()
    }
    .validate()
    .unwrap();
    assert_eq!(
        KeyEvent {
            unshifted_codepoint: Some('\0'),
            ..event()
        }
        .validate(),
        Err(InvalidKey::Scalar(0))
    );
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
        KeyEvent { text, ..event() }.validate().unwrap();
    }
    for code in (0..=0x1f).chain([0x7f]).chain(0xf700..=0xf8ff) {
        let c = char::from_u32(code).unwrap();
        let text = format!("ok{c}tail");
        assert_eq!(
            KeyEvent {
                text: &text,
                ..event()
            }
            .validate(),
            Err(InvalidKey::TextCodepoint(c))
        );
    }
    let text = "x".repeat(LIMIT);
    KeyEvent {
        text: &text,
        ..event()
    }
    .validate()
    .unwrap();
    let oversize = text + "x";
    let error = KeyEvent {
        text: &oversize,
        ..event()
    }
    .validate()
    .unwrap_err();
    assert_eq!(error, InvalidKey::TextLimit(LIMIT + 1));
    assert_eq!(error.native_code(), -6);
}

#[cfg(sf_key_public_probe)]
#[test]
fn exhaustive_compiled_public_c_probe_and_real_encoder_comparison() {
    unsafe extern "C" {
        fn sf_key_public_probe() -> i32;
    }
    // SAFETY: the opt-in probe is compiled with this exact adapter and pinned
    // public headers/library. It owns and frees every native test object.
    let line = unsafe { sf_key_public_probe() };
    assert_eq!(line, 0, "public C key probe failed at C source line {line}");
}
