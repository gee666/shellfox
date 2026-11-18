use serde_json::{json, Value};
use shellfox_protocol::observer_v2::HostEvent;
use shellfox_protocol::{observer_v2::*, *};
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/observer_v2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
#[test]
fn reply_branches_do_not_accept_null_cross_fields_or_v1_errors() {
    let original: Value = serde_json::from_slice(&bytes("pair-selection")).unwrap();
    for value in [
        Value::Null,
        json!({"code":"INTERNAL","message":"x","retryable":false}),
    ] {
        let mut invalid = original.clone();
        invalid["error"] = value;
        assert!(decode::<observer_v2::Reply<PairSelection>>(
            &serde_json::to_vec(&invalid).unwrap()
        )
        .is_err());
    }
    let mut error: Value = serde_json::from_slice(&bytes("profile-error")).unwrap();
    error["value"] = Value::Null;
    assert!(decode::<observer_v2::Reply<Discovery>>(&serde_json::to_vec(&error).unwrap()).is_err());
}
#[test]
fn safe_integer_epoch_and_canonical_base64_bounds_are_enforced() {
    let attached: BoundReply<Attachment> = decode(&bytes("attachment")).unwrap();
    let observer_v2::Reply::Success(Attachment::Attached {
        delivery,
        mut recovery,
        ..
    }) = attached.outcome
    else {
        panic!()
    };
    recovery.data = "AAAA".repeat(MAX_RECOVERY_BYTES / 3 + 1);
    assert!(recovery.validate().is_err());
    recovery.data = "AB==".into();
    assert!(recovery.validate().is_err());
    let mut delivery = delivery;
    delivery.delivery_epoch = 0;
    assert!(delivery.validate().is_err());
    delivery.delivery_epoch = MAX_SAFE_INTEGER + 1;
    assert!(delivery.validate().is_err());
    assert!("00000000-0000-0000-0000-000000000000"
        .parse::<Id>()
        .is_err());
    let wrong = Event::Data {
        delivery,
        sequence: MAX_SAFE_INTEGER + 1,
        encoding: Encoding::Base64,
        data: "AQ==".into(),
    };
    assert!(wrong.validate().is_err());
}
#[test]
fn polling_is_bounded_by_decoded_bytes_as_well_as_event_count() {
    let attached: BoundReply<Attachment> = decode(&bytes("attachment")).unwrap();
    let observer_v2::Reply::Success(Attachment::Attached { delivery, .. }) = attached.outcome
    else {
        panic!()
    };
    let data = encode_chunk(&vec![0; MAX_DATA_BYTES]).unwrap();
    let event = HostEvent::Terminal {
        event: Event::Data {
            delivery,
            sequence: 1,
            encoding: Encoding::Base64,
            data,
        },
    };
    let mut poll = observer_v2::PollResponse {
        events: vec![event; 16],
    };
    poll.validate().unwrap();
    poll.events.push(poll.events[0].clone());
    assert!(poll.validate().is_err());
    let changed = HostEvent::Changed {
        event: ChangedEvent {
            revision: 1,
            reason: ChangeReason::Sessions,
            select_session_id: None,
        },
    };
    let poll = observer_v2::PollResponse {
        events: vec![changed; 1025],
    };
    assert!(poll.validate().is_err());
}
#[test]
fn advertised_identifiers_are_bounded_but_selected_identifiers_remain_exact() {
    let mut raw: Value = serde_json::from_slice(&bytes("discovery")).unwrap();
    raw["value"]["profiles"] = json!(["future-v3", "observer-v2"]);
    let reply: observer_v2::Reply<Discovery> = decode(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(matches!(reply,observer_v2::Reply::Success(v) if v.supports_observer()));
    raw["value"]["profiles"] = json!(["observer-v2", "observer-v2"]);
    assert!(decode::<observer_v2::Reply<Discovery>>(&serde_json::to_vec(&raw).unwrap()).is_err());
    let mut offer: Value = serde_json::from_slice(&bytes("pair-offer")).unwrap();
    offer["recoveryFormat"] = json!("visible-screen-v1");
    assert!(serde_json::from_value::<PairOffer>(offer).is_err());
}
