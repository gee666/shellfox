use super::*;
use shellfox_ghostty_vt::{Key, KeyEvent};
#[test]
fn actual_bracketed_paste_payload_and_encoded_caps_are_distinct_and_overflow_has_zero_prewrite_side_effects(
) {
    assert_eq!(MAX_INPUT_PAYLOAD_BYTES, 65536);
    assert_eq!(MAX_ENCODED_INPUT_BYTES, 65536);
    let mut m = Manager::start("--child-input-boundary", "RUNTIME_INPUT_BOUNDARY_READY");
    let lease = m.lease();
    let snapshot = m.recover();
    m.ack(&snapshot).unwrap();
    assert!(
        matches!(
            m.request(Command::Paste {
                lease,
                text: "x".into()
            })
            .unwrap(),
            Response::Input(InputReceipt {
                accepted_bytes: 13,
                kind: InputKind::Paste { nonempty: true }
            })
        ),
        "actual source must enable bracketed paste"
    );
    assert!(
        matches!(m.request(Command::Key{lease,event:KeyEvent{key:Key::Character('x'),text:Some("x".into()),unshifted_codepoint:u32::from('x'),..Default::default()}}).unwrap(),Response::Input(InputReceipt{accepted_bytes:count,..}) if count>1),
        "actual source must enable keyboard encoding"
    );
    let id = RequestId(m.host.next);
    let accepted = match m
        .request(Command::Paste {
            lease,
            text: "p".repeat(MAX_INPUT_PAYLOAD_BYTES - 12),
        })
        .unwrap()
    {
        Response::Input(receipt) => receipt,
        other => panic!("{other:?}"),
    };
    assert_eq!(accepted.accepted_bytes as usize, MAX_ENCODED_INPUT_BYTES);
    let input = m.probe.input.lock().unwrap();
    let prepared = input.last().unwrap();
    assert_eq!(
        (prepared.0, prepared.1),
        (id, InputKind::Paste { nonempty: true })
    );
    assert_eq!(prepared.2.len(), MAX_ENCODED_INPUT_BYTES);
    assert!(prepared.2.starts_with(b"\x1b[200~"));
    assert!(prepared.2.ends_with(b"\x1b[201~"));
    assert!(prepared.2[6..prepared.2.len() - 6]
        .iter()
        .all(|byte| *byte == b'p'));
    let before = input.len();
    drop(input);
    for size in [MAX_INPUT_PAYLOAD_BYTES - 11, MAX_INPUT_PAYLOAD_BYTES] {
        let error = m
            .request(Command::Paste {
                lease,
                text: "q".repeat(size),
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceLimit);
        assert_eq!(
            m.probe.input.lock().unwrap().len(),
            before,
            "overflow must reject before hook or any native write"
        );
        assert_eq!(m.host.runtime.phase(), Phase::Active);
    }
    let error = m
        .request(Command::Key {
            lease,
            event: KeyEvent {
                key: Key::Character('x'),
                text: Some("x".repeat(MAX_INPUT_PAYLOAD_BYTES)),
                unshifted_codepoint: u32::from('x'),
                ..Default::default()
            },
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourceLimit);
    assert_eq!(
        m.probe.input.lock().unwrap().len(),
        before,
        "keyboard expansion must not be clipped or written as a prefix"
    );
    m.request(Command::CheckControllerReady { lease }).unwrap();
}
