use shellfox_ghostty_vt::{
    Error, Key, KeyAction, KeyActionV2, KeyEvent, KeyEventV2, Modifiers, ModifiersV2, PhysicalKey,
    Terminal,
};

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
fn all_physical_keys_and_actions_reach_real_public_encoder() {
    for mode in [b"".as_slice(), b"\x1b[>31u".as_slice()] {
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal.feed(mode).unwrap();
        for physical in PhysicalKey::ALL {
            for action in [
                KeyActionV2::Press,
                KeyActionV2::Repeat,
                KeyActionV2::Release,
            ] {
                terminal
                    .encode_key_v2(&KeyEventV2 {
                        action,
                        ..event(physical, "")
                    })
                    .unwrap();
            }
        }
    }
}

#[test]
fn all_modifier_patterns_are_accepted_even_right_only_releases() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    terminal.feed(b"\x1b[>31u").unwrap();
    for bits in 0..=0x3ff {
        for action in [
            KeyActionV2::Press,
            KeyActionV2::Repeat,
            KeyActionV2::Release,
        ] {
            let key = KeyEventV2 {
                action,
                modifiers: ModifiersV2::try_from_bits(bits).unwrap(),
                consumed_modifiers: ModifiersV2::try_from_bits(bits ^ 0x3ff).unwrap(),
                unshifted_codepoint: Some('a'),
                ..event(PhysicalKey::KeyA, "a")
            };
            terminal.encode_key_v2(&key).unwrap();
        }
    }
    terminal
        .encode_key_v2(&KeyEventV2 {
            action: KeyActionV2::Release,
            modifiers: ModifiersV2::SHIFT_SIDE,
            consumed_modifiers: ModifiersV2::ALT_SIDE,
            ..event(PhysicalKey::ShiftRight, "")
        })
        .unwrap();
}

#[test]
fn existing_safe_key_shapes_and_valid_outputs_stay_unchanged() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    let mut old = KeyEvent {
        key: Key::Character('c'),
        text: Some("c".into()),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut new = KeyEventV2 {
        modifiers: ModifiersV2::CONTROL,
        ..event(PhysicalKey::KeyC, "c")
    };
    assert_eq!(terminal.encode_key(&old).unwrap(), b"\x03");
    assert_eq!(
        terminal.encode_key(&old).unwrap(),
        terminal.encode_key_v2(&new).unwrap()
    );
    old.action = KeyAction::Repeat;
    new.action = KeyActionV2::Repeat;
    assert_eq!(
        terminal.encode_key(&old).unwrap(),
        terminal.encode_key_v2(&new).unwrap()
    );
    old.action = KeyAction::Release;
    new.action = KeyActionV2::Release;
    assert!(terminal.encode_key_v2(&new).unwrap().is_empty());
    assert_eq!(
        terminal.encode_key(&old).unwrap(),
        terminal.encode_key_v2(&new).unwrap()
    );
    for (old_key, physical) in [
        (Key::Enter, PhysicalKey::Enter),
        (Key::Tab, PhysicalKey::Tab),
        (Key::ArrowUp, PhysicalKey::ArrowUp),
        (Key::F(25), PhysicalKey::F25),
    ] {
        let old = KeyEvent {
            key: old_key,
            ..Default::default()
        };
        let new = event(physical, "");
        assert_eq!(
            terminal.encode_key(&old).unwrap(),
            terminal.encode_key_v2(&new).unwrap()
        );
    }
    assert_eq!(
        terminal
            .encode_key_v2(&event(PhysicalKey::ArrowUp, ""))
            .unwrap(),
        b"\x1b[A"
    );
    terminal.feed(b"\x1b[?1h").unwrap();
    assert_eq!(
        terminal
            .encode_key_v2(&event(PhysicalKey::ArrowUp, ""))
            .unwrap(),
        b"\x1bOA"
    );
}

#[test]
fn layout_text_scalar_composition_and_consumption_are_not_guessed() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    for text in ["é", "a\u{301}😀", "\u{80}\u{85}\u{9f}"] {
        // An explicit unavailable physical identity is legitimate for commits.
        assert_eq!(
            terminal
                .encode_key_v2(&event(PhysicalKey::Unidentified, text))
                .unwrap(),
            text.as_bytes()
        );
    }
    let shifted = KeyEventV2 {
        modifiers: ModifiersV2::SHIFT,
        consumed_modifiers: ModifiersV2::SHIFT,
        unshifted_codepoint: Some('a'),
        ..event(PhysicalKey::KeyA, "A")
    };
    assert_eq!(terminal.encode_key_v2(&shifted).unwrap(), b"A");
    for mode in [b"".as_slice(), b"\x1b[>31u".as_slice()] {
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal.feed(mode).unwrap();
        let preedit = KeyEventV2 {
            composing: true,
            ..event(PhysicalKey::KeyE, "é")
        };
        assert!(terminal.encode_key_v2(&preedit).unwrap().is_empty());
    }
    assert_eq!(
        terminal.encode_key_v2(&KeyEventV2 {
            unshifted_codepoint: Some('\0'),
            ..shifted
        }),
        Err(Error(-2))
    );
    for text in ["\0", "\r", "\n", "\x1b", "\u{7f}", "\u{f700}", "\u{f8ff}"] {
        assert_eq!(
            terminal.encode_key_v2(&event(PhysicalKey::Unidentified, text)),
            Err(Error(-2))
        );
    }
    // V1 retains its previous broader text validation; V2's C1 support is additive.
    let old = KeyEvent {
        text: Some("\u{85}".into()),
        ..Default::default()
    };
    assert_eq!(terminal.encode_key(&old), Err(Error(-2)));
}

#[test]
fn wrapper_allocation_limit_is_not_runtime_write_limit_and_never_splits() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    let text = "x".repeat(64 * 1024 + 1);
    assert_eq!(
        terminal
            .encode_key_v2(&event(PhysicalKey::Unidentified, &text))
            .unwrap(),
        text.as_bytes()
    );
    let text = "x".repeat(1024 * 1024);
    assert_eq!(
        terminal
            .encode_key_v2(&event(PhysicalKey::Unidentified, &text))
            .unwrap()
            .len(),
        text.len()
    );
    let oversized = text + "x";
    assert_eq!(
        terminal.encode_key_v2(&event(PhysicalKey::Unidentified, &oversized)),
        Err(Error(-6))
    );
    terminal.feed(b"\x1b[>31u").unwrap();
    let expansion = "x".repeat(512 * 1024);
    let key = KeyEventV2 {
        unshifted_codepoint: Some('a'),
        ..event(PhysicalKey::KeyA, &expansion)
    };
    assert_eq!(terminal.encode_key_v2(&key), Err(Error(-6)));
}

#[test]
fn sticky_reply_poison_wins_over_invalid_v2_inputs() {
    let mut terminal = Terminal::new(80, 24).unwrap();
    let queries = b"\x1b[6n".repeat(120_000);
    terminal.feed(&queries).unwrap();
    assert_eq!(terminal.feed(&queries), Err(Error(-6)));
    let invalid = KeyEventV2 {
        unshifted_codepoint: Some('\0'),
        ..event(PhysicalKey::Unidentified, "\0")
    };
    assert_eq!(terminal.encode_key_v2(&invalid), Err(Error(-6)));
}
