use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use shellfox_protocol::{observer_v2::*, *};
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/observer_v2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn roundtrip<T: DeserializeOwned + Serialize + Validate>(name: &str) {
    let raw = bytes(name);
    let value: T = decode(&raw).unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        serde_json::from_slice::<Value>(&raw).unwrap()
    );
}
#[test]
fn canonical_golden_bodies_validate_and_roundtrip() {
    roundtrip::<observer_v2::Discover>("discovery-request");
    roundtrip::<observer_v2::Reply<Discovery>>("discovery");
    roundtrip::<PairOffer>("pair-offer");
    roundtrip::<observer_v2::Reply<PairSelection>>("pair-selection");
    roundtrip::<Binding>("binding");
    roundtrip::<BoundReply<Snapshot>>("snapshot");
    roundtrip::<BoundReply<Attachment>>("attachment");
    roundtrip::<BoundPoll>("events-request");
    roundtrip::<BoundReply<observer_v2::PollResponse>>("events");
    roundtrip::<BoundReply<Acknowledged>>("acknowledged");
    roundtrip::<Disconnect>("disconnect-attempt");
    roundtrip::<Disconnect>("disconnect-connection");
    roundtrip::<observer_v2::Reply<observer_v2::Disconnected>>("disconnected");
    roundtrip::<observer_v2::Reply<Discovery>>("profile-error");
    roundtrip::<Event>("source-ended");
    roundtrip::<Event>("process-exited");
    roundtrip::<Event>("layout");
    for name in [
        "request-snapshot",
        "request-attach",
        "request-ack-recovery",
        "request-ack-data",
        "request-detach",
    ] {
        let decoded = decode_request(&bytes(name)).unwrap();
        assert!(decoded.command.is_profile_allowed());
    }
}
#[test]
fn authenticated_peer_and_full_binding_are_required_not_just_connection_uuid() {
    let decoded = decode_request(&bytes("request-snapshot")).unwrap();
    let peer = AuthenticatedPeerId::new("verified-peer").unwrap();
    let connection = ConnectionBinding::new(peer.clone(), decoded.binding.clone());
    decoded.check_connection(&connection, &peer).unwrap();
    assert!(decoded
        .check_connection(
            &connection,
            &AuthenticatedPeerId::new("other-peer").unwrap()
        )
        .is_err());
    let mut wrong = decoded.binding.clone();
    wrong.client_id = Id::new();
    assert!(connection.check(&peer, &wrong).is_err());
    wrong = decoded.binding.clone();
    wrong.owner_id = Id::new();
    assert!(connection.check(&peer, &wrong).is_err());
    wrong = decoded.binding.clone();
    wrong.connection_id = Id::new();
    assert!(connection.check(&peer, &wrong).is_err());
}
#[test]
fn delivery_never_becomes_input_or_layout_authority() {
    let original: Value = serde_json::from_slice(&bytes("attachment")).unwrap();
    for (field, value) in [
        ("input", json!({"controller":1,"epoch":1})),
        ("layout", json!({"kind":"resizeExternal"})),
    ] {
        let mut wrong = original.clone();
        wrong["value"][field] = value;
        assert!(decode::<BoundReply<Attachment>>(&serde_json::to_vec(&wrong).unwrap()).is_err());
    }
    let mut missing = original;
    missing["value"].as_object_mut().unwrap().remove("input");
    assert!(decode::<BoundReply<Attachment>>(&serde_json::to_vec(&missing).unwrap()).is_err());
    let command:observer_v2::Command=serde_json::from_value(json!({"method":"writeTerminal","payload":{
        "tabId":"50000000-0000-4000-8000-000000000001","generation":"60000000-0000-4000-8000-000000000001","controlEpoch":1,"data":"x"}})).unwrap();
    command.validate().unwrap();
    assert!(command.authorize_profile().is_err());
    assert_eq!(observer_v2::METHODS, REMOTE_METHODS);
}
#[test]
fn complete_inventory_preserves_unknown_facts_and_rejects_partial_or_hidden_entries() {
    let mut raw: Value = serde_json::from_slice(&bytes("snapshot")).unwrap();
    let value: BoundReply<Snapshot> = decode(&serde_json::to_vec(&raw).unwrap()).unwrap();
    let observer_v2::Reply::Success(snapshot) = value.outcome else {
        panic!()
    };
    assert_eq!(snapshot.entries.len(), 3);
    raw["value"]["coverage"]["mode"] = json!("partial");
    assert!(decode::<BoundReply<Snapshot>>(&serde_json::to_vec(&raw).unwrap()).is_err());
    raw["value"]["coverage"]["mode"] = json!("complete");
    raw["value"]["entries"].as_array_mut().unwrap().pop();
    assert!(decode::<BoundReply<Snapshot>>(&serde_json::to_vec(&raw).unwrap()).is_err());
    assert!(serde_json::from_value::<Fact<Option<String>>>(json!({"state":"known"})).is_err());
    let null: Fact<Option<String>> =
        serde_json::from_value(json!({"state":"known","value":null})).unwrap();
    assert!(matches!(null, Fact::Known { value: None }));
}
#[test]
fn strict_v2_rejects_aliases_wrong_versions_extra_fields_and_unsafe_events() {
    for version in [0, 1, 3] {
        let mut v: Value = serde_json::from_slice(&bytes("pair-offer")).unwrap();
        v["version"] = json!(version);
        assert!(serde_json::from_value::<PairOffer>(v).is_err());
    }
    let mut request: Value = serde_json::from_slice(&bytes("request-snapshot")).unwrap();
    request["peerId"] = json!("forged");
    assert!(decode_request(&serde_json::to_vec(&request).unwrap()).is_err());
    let mut ended: Value = serde_json::from_slice(&bytes("source-ended")).unwrap();
    ended["exitCode"] = json!(0);
    assert!(serde_json::from_value::<Event>(ended).is_err());
    let mut exit: Value = serde_json::from_slice(&bytes("process-exited")).unwrap();
    exit["exitCode"] = Value::Null;
    assert!(decode::<Event>(&serde_json::to_vec(&exit).unwrap()).is_err());
    let mut raw: Value = serde_json::from_slice(&bytes("events")).unwrap();
    raw["value"]["events"][0]["event"]["data"] = json!("AB==");
    assert!(
        decode::<BoundReply<observer_v2::PollResponse>>(&serde_json::to_vec(&raw).unwrap())
            .is_err()
    );
    let mut layout: Value = serde_json::from_slice(&bytes("layout")).unwrap();
    layout["recoveryRequired"] = json!(false);
    assert!(decode::<Event>(&serde_json::to_vec(&layout).unwrap()).is_err());
}
#[test]
fn v1_paths_and_bytes_remain_separate() {
    assert_eq!(Endpoint::Discover.path(), "/v1/discover");
    assert_eq!(Endpoint::DiscoverV2.path(), "/v2/discover");
    assert_eq!(Endpoint::PairV2.path(), "/v2/pair");
    assert_eq!(Endpoint::RequestV2.path(), "/v2/request");
    assert_eq!(Endpoint::EventsV2.path(), "/v2/events");
    assert_eq!(Endpoint::DisconnectV2.path(), "/v2/disconnect");
    assert!(decode::<shellfox_protocol::Reply<TerminalAttachment>>(&bytes("attachment")).is_err());
    let v1 = std::fs::read(format!(
        "{}/../shellfox-client/tests/fixtures/terminal-attach-owned-success.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    assert!(decode::<BoundReply<Attachment>>(&v1).is_err());
}
