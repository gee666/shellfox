use super::*;
fn legacy() -> Value {
    json!({"ok":true,"value":{"service":"shellfox","protocolVersion":1}})
}
#[test]
fn only_native_404_before_pair_can_offer_validated_v1_without_pairing_it() {
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.queue(Response::Error(TransportError::Rejected { status: 404 }));
    fake.json(legacy());
    assert!(matches!(
        block_on(client.discover(FallbackPolicy::OfferLegacy)).unwrap(),
        DiscoveryOutcome::LegacyAvailable
    ));
    assert_eq!(client.status(), Status::LegacyAvailable);
    assert!(client.binding().is_none());
    let s = fake.0.lock().unwrap();
    assert_eq!(s.1.len(), 2);
    assert_eq!(s.1[0].endpoint, Endpoint::DiscoverV2);
    assert_eq!(s.1[1].endpoint, Endpoint::Discover);
}
#[test]
fn valid_unknown_advertised_profile_can_offer_legacy_but_partial_catalog_never_negotiates() {
    let fake = Fake::default();
    let client = super::client(&fake);
    let mut discovery = value("discovery");
    discovery["value"]["profiles"] = json!(["future-v3"]);
    fake.json(discovery);
    fake.json(legacy());
    assert!(matches!(
        block_on(client.discover(FallbackPolicy::OfferLegacy)).unwrap(),
        DiscoveryOutcome::LegacyAvailable
    ));
    let fake = Fake::default();
    let client = super::client(&fake);
    let mut offer = value("pair-offer");
    offer["catalogMode"] = json!("partial");
    assert!(serde_json::from_value::<wire::PairOffer>(offer).is_err());
    assert_eq!(fake.count(), 0);
    assert_eq!(client.status(), Status::Disconnected);
}
#[test]
fn fallback_never_uses_network_errors_auth_denials_malformed_json_or_invalid_legacy() {
    for error in [
        TransportError::Cancelled,
        TransportError::Deadline,
        TransportError::Unavailable,
        TransportError::Authentication,
        TransportError::Rejected { status: 403 },
    ] {
        let fake = Fake::default();
        let client = super::client(&fake);
        fake.queue(Response::Error(error));
        assert!(block_on(client.discover(FallbackPolicy::OfferLegacy)).is_err());
        assert_eq!(fake.count(), 1);
    }
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.json(json!({"version":2,"ok":true,"value":{"service":"shellfox"}}));
    assert!(matches!(
        block_on(client.discover(FallbackPolicy::OfferLegacy)),
        Err(ClientError::InvalidResponse)
    ));
    assert_eq!(fake.count(), 1);
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.queue(Response::Error(TransportError::Rejected { status: 404 }));
    fake.json(json!({"ok":true,"value":{"service":"other","protocolVersion":1}}));
    assert!(block_on(client.discover(FallbackPolicy::OfferLegacy)).is_err());
    assert_eq!(fake.count(), 2);
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.json(json!({"version":2,"ok":false,"error":{"code":"ACCESS_DENIED","message":"Denied.","retryable":false,"detail":{"kind":"application"}}}));
    assert!(matches!(
        block_on(client.discover(FallbackPolicy::OfferLegacy)),
        Err(ClientError::Host(_))
    ));
    assert_eq!(fake.count(), 1);
}
#[test]
fn fallback_is_explicit_and_never_reopened_after_pair_admission() {
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.queue(Response::Error(TransportError::Rejected { status: 404 }));
    assert!(block_on(client.discover(FallbackPolicy::Never)).is_err());
    assert_eq!(fake.count(), 1);
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.bytes("discovery");
    block_on(client.discover(FallbackPolicy::OfferLegacy)).unwrap();
    fake.queue(Response::Error(TransportError::Rejected { status: 404 }));
    assert!(block_on(client.pair(id(3))).is_err());
    assert_eq!(fake.count(), 2);
    fake.queue(Response::Error(TransportError::Rejected { status: 404 }));
    assert!(block_on(client.discover(FallbackPolicy::OfferLegacy)).is_err());
    assert_eq!(fake.count(), 3);
    assert!(fake
        .0
        .lock()
        .unwrap()
        .1
        .iter()
        .all(|call| call.endpoint != Endpoint::Discover));
}
#[test]
fn pair_echo_and_every_bound_reply_must_match_exact_owner_client_and_connection() {
    for field in ["ownerId", "clientId", "attemptId"] {
        let fake = Fake::default();
        let client = super::client(&fake);
        fake.bytes("discovery");
        block_on(client.discover(FallbackPolicy::Never)).unwrap();
        let mut selection = value("pair-selection");
        selection["value"][field] = json!(id(9));
        fake.json(selection);
        assert!(matches!(
            block_on(client.pair(id(3))),
            Err(ClientError::InvalidResponse)
        ));
        assert!(client.binding().is_none());
    }
    for field in ["ownerId", "clientId", "connectionId"] {
        let fake = Fake::default();
        let client = connected(&fake);
        let mut response = value("snapshot");
        response["binding"][field] = json!(id(9));
        fake.json(response);
        assert!(matches!(
            block_on(client.execute(&wire::GetSnapshot::from(GetSnapshot {}))),
            Err(ClientError::InvalidResponse)
        ));
        assert!(client.binding().is_none());
        assert_eq!(client.status(), Status::Reconnecting);
    }
}
