use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use shellfox_protocol::{manager_v2 as m, observer_v2 as o, *};
#[path = "support/json_presence.rs"]
mod json_presence;
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/manager_v2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn round<T: DeserializeOwned + Serialize + Validate>(name: &str) {
    let raw = bytes(name);
    let value: T = decode(&raw).unwrap();
    json_presence::assert_same(
        &serde_json::to_value(value).unwrap(),
        &serde_json::from_slice::<Value>(&raw).unwrap(),
    );
}
#[test]
fn canonical_handshake_and_real_ref_examples_roundtrip() {
    round::<m::Reply<m::Discovery>>("discovery");
    round::<m::PairOffer>("pair-offer");
    round::<m::Reply<m::PairSelection>>("pair-selection");
    round::<m::Binding>("binding");
    round::<m::BoundReply<m::Attachment>>("attachment");
    round::<m::BoundReply<m::Attachment>>("attachment-busy");
    round::<m::BoundReply<m::Attachment>>("attachment-intrinsic");
    round::<m::BoundReply<m::AcceptedInput>>("accepted-bytes");
    round::<m::BoundReply<m::AcceptedInput>>("accepted-mouse-zero");
    round::<m::BoundReply<m::PollResponse>>("events");
    round::<m::BoundReply<m::Resized>>("resized");
    round::<m::Disconnect>("disconnect-attempt");
    round::<m::Disconnect>("disconnect-connection");
    round::<m::BoundReply<m::AcceptedInput>>("input-outcome-unknown");
    round::<m::BoundReply<m::AcceptedInput>>("stale-layout");
    round::<m::Event>("revoked-control");
    for name in [
        "request-attach",
        "request-takeover",
        "request-ack-recovery",
        "request-ack-data",
        "request-resize",
        "request-write-bytes",
        "request-write-key",
        "request-write-paste",
        "request-write-mouse",
        "request-detach",
    ] {
        m::decode_request(&bytes(name)).unwrap();
    }
    assert_eq!(m::METHODS, REMOTE_METHODS);
}
#[test]
fn pair_policy_is_fixed_write_fullaccount_and_offer_cannot_request_permissions() {
    let mut offer: Value = serde_json::from_slice(&bytes("pair-offer")).unwrap();
    offer["permission"] = json!("write");
    assert!(serde_json::from_value::<m::PairOffer>(offer).is_err());
    let mut selection: Value = serde_json::from_slice(&bytes("pair-selection")).unwrap();
    selection["value"]["devicePermission"] = json!("read-only");
    assert!(
        decode::<m::Reply<m::PairSelection>>(&serde_json::to_vec(&selection).unwrap()).is_err()
    );
    assert!(decode::<o::Reply<o::PairSelection>>(&bytes("pair-selection")).is_err());
    assert!(o::decode_request(&bytes("request-write-bytes")).is_err());
}
#[test]
fn refs_do_not_mint_control_from_delivery_or_intrinsic_state() {
    let original: Value = serde_json::from_slice(&bytes("attachment")).unwrap();
    for field in ["controlEpoch", "generation", "tabId"] {
        let mut raw = original.clone();
        raw["value"]["input"][field] = if field == "controlEpoch" {
            json!(8)
        } else {
            json!("90000000-0000-4000-8000-000000000001")
        };
        assert!(
            decode::<m::BoundReply<m::Attachment>>(&serde_json::to_vec(&raw).unwrap()).is_err()
        );
    }
    let mut raw = original;
    raw["value"]["inputState"] = json!("intrinsicReadOnly");
    assert!(decode::<m::BoundReply<m::Attachment>>(&serde_json::to_vec(&raw).unwrap()).is_err());
}
#[test]
fn native_grid_and_ack_stamp_are_not_observer_bounds_or_scalar_ack() {
    let mut raw: Value = serde_json::from_slice(&bytes("attachment")).unwrap();
    raw["value"]["recovery"]["cols"] = json!(1000);
    raw["value"]["recovery"]["rows"] = json!(1000);
    decode::<m::BoundReply<m::Attachment>>(&serde_json::to_vec(&raw).unwrap()).unwrap();
    raw["value"]["recovery"]["rows"] = json!(2000);
    assert!(decode::<m::BoundReply<m::Attachment>>(&serde_json::to_vec(&raw).unwrap()).is_err());
    let mut ack: Value = serde_json::from_slice(&bytes("request-ack-recovery")).unwrap();
    ack["payload"]["ack"]
        .as_object_mut()
        .unwrap()
        .remove("layoutRevision");
    assert!(m::decode_request(&serde_json::to_vec(&ack).unwrap()).is_err());
    let mut resize: Value = serde_json::from_slice(&bytes("request-resize")).unwrap();
    resize["payload"]
        .as_object_mut()
        .unwrap()
        .remove("expectedLayoutRevision");
    assert!(m::decode_request(&serde_json::to_vec(&resize).unwrap()).is_err());
}
#[test]
fn input_bytes_and_capture_are_exact_and_receipts_are_actual_correlated() {
    let request = m::decode_request(&bytes("request-write-bytes")).unwrap();
    let m::Command::WriteTerminal(input) = request.command else {
        panic!()
    };
    assert_eq!(
        input.input.decoded_bytes().unwrap().unwrap(),
        [0x80, 0, 0xff, 13]
    );
    assert_eq!(
        input.hints.settlement_capture(),
        m::SettlementCapture::Captured(None)
    );
    let reply: m::BoundReply<m::AcceptedInput> = decode(&bytes("accepted-bytes")).unwrap();
    let m::Reply::Success(mut receipt) = reply.outcome else {
        panic!()
    };
    receipt.check_input(&input).unwrap();
    receipt.accepted_bytes = 3;
    assert!(receipt.check_input(&input).is_err());
    let absent: m::InputHints =
        serde_json::from_value(json!({"userInitiated":true,"activityHint":true})).unwrap();
    assert_eq!(
        absent.settlement_capture(),
        m::SettlementCapture::Unavailable
    );
    assert!(serde_json::to_value(absent)
        .unwrap()
        .get("settledAt")
        .is_none());
    let mut bad = serde_json::to_value(&input).unwrap();
    bad["input"]["data"] = json!("AB==");
    assert!(serde_json::from_value::<m::WriteTerminal>(bad)
        .unwrap()
        .validate()
        .is_err());
}
#[test]
fn payload_and_encoded_receipt_budgets_are_separate_without_expanding_io() {
    assert_eq!(m::MAX_INPUT_PAYLOAD_BYTES, 65536);
    assert_eq!(m::MAX_ENCODED_INPUT_BYTES, 65536);
    let decoded = m::decode_request(&bytes("request-write-bytes")).unwrap();
    let m::Command::WriteTerminal(mut request) = decoded.command else {
        panic!()
    };
    request.input = m::Input::Paste {
        text: "x".repeat(m::MAX_INPUT_PAYLOAD_BYTES),
    };
    request.validate().unwrap();
    let response = m::AcceptedInput {
        input_id: request.input_id,
        kind: m::InputKind::Paste,
        accepted_bytes: 65536,
        disposition: m::Disposition::Accepted,
    };
    response.check_input(&request).unwrap();
    let too_large = m::AcceptedInput {
        accepted_bytes: 65537,
        ..response
    };
    assert!(too_large.validate().is_err());
    let error:m::Error=serde_json::from_value(json!({"code":"VALIDATION","message":"Owner encoding exceeds the encoded write budget.","retryable":false,"detail":{"kind":"resourceLimit","limit":"encodedInputBytes"}})).unwrap();
    error.validate().unwrap();
    // This is a protocol-bound check, not proof of owner encoding. Runtime tests use the actual mode-enabled Ghostty encoder and verify no writer/hook on overflow.
}
#[test]
fn canonical_key_uses_full_physical_state_and_rejects_old_provisional_shapes() {
    let request = m::decode_request(&bytes("request-write-key")).unwrap();
    let m::Command::WriteTerminal(request) = request.command else {
        panic!()
    };
    let m::Input::Key { event } = request.input else {
        panic!()
    };
    assert_eq!(event.physical, m::PhysicalKey::Space);
    assert_eq!(event.modifiers.bits(), 2);
    assert_eq!(event.text, "");
    assert_eq!(event.unshifted_char(), Some(' '));
    let mut raw: Value = serde_json::from_slice(&bytes("request-write-key")).unwrap();
    raw["payload"]["input"]["event"] = json!({"key":{"kind":"character","value":" "},"action":"press","modifiers":{"shift":false,"ctrl":true,"alt":false,"super":false},"consumedModifiers":{"shift":false,"ctrl":false,"alt":false,"super":false},"text":null,"unshiftedCodepoint":32,"composing":false});
    assert!(m::decode_request(&serde_json::to_vec(&raw).unwrap()).is_err());
}
#[test]
fn mouse_is_structured_and_geometry_errors_never_become_bytes() {
    let request = m::decode_request(&bytes("request-write-mouse")).unwrap();
    let m::Command::WriteTerminal(input) = request.command else {
        panic!()
    };
    assert!(matches!(input.input, m::Input::Mouse { .. }));
    let mut raw = serde_json::to_value(input).unwrap();
    raw["input"]["event"]["xPx"] = json!(1000000001);
    assert!(serde_json::from_value::<m::WriteTerminal>(raw)
        .unwrap()
        .validate()
        .is_err());
    let mut bytes_req: Value = serde_json::from_slice(&bytes("request-write-bytes")).unwrap();
    bytes_req["payload"]["input"]["origin"] = json!("mouse");
    assert!(m::decode_request(&serde_json::to_vec(&bytes_req).unwrap()).is_err());
}

#[test]
fn native0_wire_is_typed_bounded_and_sender_is_pair_selection_not_body_authority() {
    // Header-only DTO fixture: NOT a decodable NativeSnapshot or native proof.
    round::<m::Recovery>("recovery-native0");
    let recovery: m::Recovery = decode(&bytes("recovery-native0")).unwrap();
    assert_eq!(recovery.bytes().unwrap(), b"GHOSTSNP\x01\0");
    let m::Recovery::Native0RawV1 { metadata, .. } = &recovery else {
        panic!()
    };
    let mut offer: m::PairOffer = decode(&bytes("pair-offer")).unwrap();
    offer.recovery_format = m::RecoveryFormat::Native0RawV1;
    offer.native0_receiver = Some(metadata.codec.clone());
    offer.validate().unwrap();
    let reply: m::Reply<m::PairSelection> = decode(&bytes("pair-selection")).unwrap();
    let m::Reply::Success(mut selected) = reply else {
        panic!()
    };
    selected.recovery_format = offer.recovery_format;
    selected.native0_receiver = offer.native0_receiver.clone();
    selected.native0_sender = Some(metadata.codec.clone());
    selected.validate_offer(&offer).unwrap();
    recovery.check_selection(&selected).unwrap();
    selected.native0_sender.as_mut().unwrap().engine_identity =
        m::EngineIdentity::try_from("f".repeat(64)).unwrap();
    assert!(recovery.check_selection(&selected).is_err());
    let mut wrong = serde_json::to_value(&recovery).unwrap();
    wrong["metadata"]["state"]["terminalProfile"] = json!(1);
    assert!(decode::<m::Recovery>(&serde_json::to_vec(&wrong).unwrap()).is_err());
    wrong = serde_json::to_value(&recovery).unwrap();
    wrong["metadata"]["limits"]["maxAllocationBytes"] = json!(u64::MAX);
    assert!(decode::<m::Recovery>(&serde_json::to_vec(&wrong).unwrap()).is_err());
    let old: Value = serde_json::from_slice(&bytes("attachment")).unwrap();
    assert!(decode::<o::BoundReply<o::Attachment>>(&serde_json::to_vec(&old).unwrap()).is_err());
}
#[test]
fn formatted_wire_does_not_accept_native_metadata_or_arbitrary_binary() {
    let attachment: m::BoundReply<m::Attachment> = decode(&bytes("attachment")).unwrap();
    let m::Reply::Success(m::Attachment::Attached(value)) = attachment.outcome else {
        panic!()
    };
    assert_eq!(
        value.recovery.format(),
        m::RecoveryFormat::FormattedActiveBuffer
    );
    let mut recovery = serde_json::to_value(value.recovery).unwrap();
    recovery["metadata"] = json!({});
    assert!(decode::<m::Recovery>(&serde_json::to_vec(&recovery).unwrap()).is_err());
}
