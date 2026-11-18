use super::*;
use shellfox_ghostty_vt::Terminal;
fn event(physical: PhysicalKey, text: &str) -> KeyEventV2<'_> {
    KeyEventV2 {
        physical,
        action: KeyActionV2::Press,
        modifiers: ModifiersV2::NONE,
        consumed_modifiers: ModifiersV2::NONE,
        text,
        unshifted_codepoint: None,
        composing: false,
    }
}
#[test]
fn all_176_checked_physical_values_and_actions_survive_owned_conversion_and_real_encoding() {
    assert_eq!(PhysicalKey::ALL.len(), 176);
    for mode in [b"".as_slice(), b"\x1b[>31u".as_slice()] {
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal.feed(mode).unwrap();
        for physical in PhysicalKey::ALL {
            assert_eq!(PhysicalKey::try_from(physical.code()).unwrap(), physical);
            for action in [
                KeyActionV2::Press,
                KeyActionV2::Repeat,
                KeyActionV2::Release,
            ] {
                let original = KeyEventV2 {
                    action,
                    modifiers: ModifiersV2::CAPS_LOCK | ModifiersV2::ALT_SIDE,
                    consumed_modifiers: ModifiersV2::SHIFT_SIDE,
                    unshifted_codepoint: Some('ä'),
                    ..event(physical, "é🦊")
                };
                let owned = OwnedKeyEventV2::from(original);
                let borrowed = owned.borrowed();
                assert_eq!(borrowed.physical, original.physical);
                assert_eq!(borrowed.action, action);
                assert_eq!(borrowed.modifiers, original.modifiers);
                assert_eq!(borrowed.consumed_modifiers, original.consumed_modifiers);
                assert_eq!(borrowed.text, original.text);
                assert_eq!(borrowed.unshifted_codepoint, Some('ä'));
                assert!(!borrowed.composing);
                assert_eq!(
                    terminal.encode_key_v2(&original).unwrap(),
                    terminal.encode_key_v2(&borrowed).unwrap()
                );
            }
        }
    }
    for invalid in [176, 177, u32::MAX] {
        assert_eq!(
            PhysicalKey::try_from(invalid),
            Err(InvalidKeyV2::PhysicalKey(invalid))
        );
    }
}
#[test]
fn full_lock_and_side_masks_are_preserved_without_reconstruction_or_pre_subtraction() {
    for bits in 0..=0x3ff {
        let original = KeyEventV2 {
            action: KeyActionV2::Release,
            modifiers: ModifiersV2::try_from_bits(bits).unwrap(),
            consumed_modifiers: ModifiersV2::try_from_bits(bits ^ 0x3ff).unwrap(),
            ..event(PhysicalKey::ShiftRight, "")
        };
        let owned = OwnedKeyEventV2::from(original);
        assert_eq!(owned.modifiers.bits(), bits as u16);
        assert_eq!(owned.consumed_modifiers.bits(), (bits ^ 0x3ff) as u16);
        assert_eq!(owned.borrowed().modifiers, original.modifiers);
    }
    assert!(ModifiersV2::try_from_bits(1 << 10).is_err());
}
#[test]
fn real_encoder_keeps_keypad_ordinary_locks_and_physical_sides_distinct() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    terminal.feed(b"\x1b[>31u").unwrap();
    for (ordinary, keypad, scalar) in [
        (PhysicalKey::Enter, PhysicalKey::NumpadEnter, None),
        (PhysicalKey::Digit1, PhysicalKey::Numpad1, Some('1')),
        (PhysicalKey::Equal, PhysicalKey::NumpadAdd, Some('=')),
        (PhysicalKey::ShiftLeft, PhysicalKey::ShiftRight, None),
    ] {
        let a = OwnedKeyEventV2::from(KeyEventV2 {
            unshifted_codepoint: scalar,
            ..event(ordinary, "")
        });
        let b = OwnedKeyEventV2::from(KeyEventV2 {
            unshifted_codepoint: scalar,
            ..event(keypad, "")
        });
        assert_ne!(
            terminal.encode_key_v2(&a.borrowed()).unwrap(),
            terminal.encode_key_v2(&b.borrowed()).unwrap(),
            "{ordinary:?} must not collapse into {keypad:?}"
        );
    }
    let base = KeyEventV2 {
        unshifted_codepoint: Some('a'),
        ..event(PhysicalKey::KeyA, "a")
    };
    let normal = terminal.encode_key_v2(&base).unwrap();
    for lock in [ModifiersV2::CAPS_LOCK, ModifiersV2::NUM_LOCK] {
        let owned = OwnedKeyEventV2::from(KeyEventV2 {
            modifiers: lock,
            ..base
        });
        assert_ne!(terminal.encode_key_v2(&owned.borrowed()).unwrap(), normal);
    }
    let release = OwnedKeyEventV2::from(KeyEventV2 {
        action: KeyActionV2::Release,
        ..base
    });
    assert_ne!(terminal.encode_key_v2(&release.borrowed()).unwrap(), normal);
    let composing = OwnedKeyEventV2::from(KeyEventV2 {
        composing: true,
        ..base
    });
    assert!(terminal
        .encode_key_v2(&composing.borrowed())
        .unwrap()
        .is_empty());
}
#[test]
fn text_is_owned_across_fifo_and_optional_scalar_is_not_normalized() {
    let mut original = String::from("éa\u{301}🦊");
    let mut owned = OwnedKeyEventV2::from(event(PhysicalKey::IntlYen, &original));
    original.clear();
    original.push_str("new UI state");
    drop(original);
    assert_eq!(owned.text, "éa\u{301}🦊");
    assert_eq!(owned.physical, PhysicalKey::IntlYen);
    assert_eq!(owned.unshifted_codepoint, None);
    let mut terminal = Terminal::new(80, 24).unwrap();
    owned.unshifted_codepoint = Some('\0');
    assert_eq!(
        terminal.encode_key_v2(&owned.borrowed()),
        Err(shellfox_ghostty_vt::Error(-2))
    );
}
