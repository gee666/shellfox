use super::*;
use shellfox_ghostty_vt::{KeyEventV2, Terminal};
fn event(physical: PhysicalKey, text: &str) -> OwnedKeyEventV2 {
    OwnedKeyEventV2 {
        physical,
        action: KeyActionV2::Press,
        modifiers: ModifiersV2::NONE,
        consumed_modifiers: ModifiersV2::NONE,
        text: text.into(),
        unshifted_codepoint: None,
        composing: false,
    }
}
fn encoded(event: &OwnedKeyEventV2) -> Vec<u8> {
    // Test-only reference encoder. Production still has one authoritative parser.
    let mut terminal = Terminal::new(80, 24).unwrap();
    terminal.feed(b"\x1b[?2004h\x1b[>31u").unwrap();
    terminal
        .encode_key_v2(&KeyEventV2 {
            physical: event.physical,
            action: event.action,
            modifiers: event.modifiers,
            consumed_modifiers: event.consumed_modifiers,
            text: &event.text,
            unshifted_codepoint: event.unshifted_codepoint,
            composing: event.composing,
        })
        .unwrap()
}
fn receipt(response: Response) -> InputReceipt {
    match response {
        Response::Input(receipt) => receipt,
        other => panic!("{other:?}"),
    }
}
#[test]
fn owned_key_v2_crosses_real_fifo_without_text_or_modifier_recapture_and_reports_actual_write() {
    let mut m = Manager::start("--child-input-boundary", "RUNTIME_INPUT_BOUNDARY_READY");
    let lease = m.lease();
    let snapshot = m.recover();
    m.ack(&snapshot).unwrap();
    let mut ui_text = String::from("a\u{301}🦊");
    let owned = OwnedKeyEventV2::from(KeyEventV2 {
        physical: PhysicalKey::KeyA,
        action: KeyActionV2::Repeat,
        modifiers: ModifiersV2::SHIFT
            | ModifiersV2::CAPS_LOCK
            | ModifiersV2::NUM_LOCK
            | ModifiersV2::ALT_SIDE,
        consumed_modifiers: ModifiersV2::SHIFT | ModifiersV2::CONTROL_SIDE,
        text: &ui_text,
        unshifted_codepoint: Some('a'),
        composing: false,
    });
    let expected = encoded(&owned);
    assert!(!expected.is_empty());
    let id = m.host.send(Command::KeyV2 {
        lease,
        event: owned,
    });
    ui_text.clear();
    ui_text.push_str("new UI state after FIFO admission");
    drop(ui_text);
    let actual = receipt(m.reply(id).unwrap());
    assert_eq!(
        actual,
        InputReceipt {
            accepted_bytes: expected.len() as u32,
            kind: InputKind::Key {
                action: shellfox_ghostty_vt::KeyAction::Repeat,
                has_committed_text: true
            }
        }
    );
    assert_eq!(
        m.probe.input.lock().unwrap().as_slice(),
        &[(id, actual.kind, expected)]
    );
    for physical in [
        PhysicalKey::Enter,
        PhysicalKey::NumpadEnter,
        PhysicalKey::Digit1,
        PhysicalKey::Numpad1,
        PhysicalKey::Equal,
        PhysicalKey::NumpadAdd,
        PhysicalKey::ShiftLeft,
        PhysicalKey::ShiftRight,
        PhysicalKey::IntlRo,
    ] {
        let mut key = event(physical, "");
        key.unshifted_codepoint = Some('1');
        let expected = encoded(&key);
        let id = RequestId(m.host.next);
        let actual = receipt(m.request(Command::KeyV2 { lease, event: key }).unwrap());
        assert_eq!(actual.accepted_bytes as usize, expected.len());
        if !expected.is_empty() {
            let input = m.probe.input.lock().unwrap();
            assert_eq!(input.last().unwrap(), &(id, actual.kind, expected));
        }
    }
    let mut release = event(PhysicalKey::ShiftRight, "");
    release.action = KeyActionV2::Release;
    release.modifiers = ModifiersV2::SHIFT_SIDE;
    release.consumed_modifiers = ModifiersV2::ALT_SIDE;
    let expected = encoded(&release);
    assert!(!expected.is_empty());
    let actual = receipt(
        m.request(Command::KeyV2 {
            lease,
            event: release,
        })
        .unwrap(),
    );
    assert_eq!(
        actual.kind,
        InputKind::Key {
            action: shellfox_ghostty_vt::KeyAction::Release,
            has_committed_text: false
        }
    );
    assert_eq!(actual.accepted_bytes as usize, expected.len());
    assert_eq!(m.probe.input.lock().unwrap().last().unwrap().2, expected);
}
#[test]
fn key_v2_preserves_real_stamp_gate_composition_zero_and_encoded_cap_before_any_hook_or_write() {
    let mut m = Manager::start("--child-input-boundary", "RUNTIME_INPUT_BOUNDARY_READY");
    let lease = m.lease();
    let snapshot = m.recover();
    let mut key = event(PhysicalKey::KeyA, "a");
    key.unshifted_codepoint = Some('a');
    assert_eq!(
        m.request(Command::KeyV2 {
            lease,
            event: key.clone()
        })
        .unwrap_err()
        .code,
        ErrorCode::RecoveryRequired
    );
    assert!(m.probe.input.lock().unwrap().is_empty());
    m.ack(&snapshot).unwrap();
    let mut preedit = key.clone();
    preedit.composing = true;
    preedit.text = "é".into();
    assert_eq!(
        receipt(
            m.request(Command::KeyV2 {
                lease,
                event: preedit
            })
            .unwrap()
        ),
        InputReceipt {
            accepted_bytes: 0,
            kind: InputKind::Key {
                action: shellfox_ghostty_vt::KeyAction::Press,
                has_committed_text: false
            }
        }
    );
    assert!(m.probe.input.lock().unwrap().is_empty());
    key.text = "x".repeat(MAX_INPUT_PAYLOAD_BYTES);
    assert_eq!(
        m.request(Command::KeyV2 { lease, event: key })
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    assert!(m.probe.input.lock().unwrap().is_empty());
    let mut invalid = event(PhysicalKey::IntlYen, "a");
    invalid.unshifted_codepoint = Some('\0');
    assert_eq!(
        m.request(Command::KeyV2 {
            lease,
            event: invalid
        })
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    assert!(m.probe.input.lock().unwrap().is_empty());
    m.request(Command::CheckControllerReady { lease }).unwrap();
    assert_eq!(m.host.runtime.phase(), Phase::Active);
}
