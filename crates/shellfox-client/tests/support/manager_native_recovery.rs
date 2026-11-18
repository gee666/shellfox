//! Transport/policy fixtures only, not actual Native0 binaries or persisted authority.
use super::*;
fn codec(digit: &str) -> m::Native0Codec {
    m::Native0Codec::new(m::EngineIdentity::try_from(digit.repeat(64)).unwrap())
}
fn native_responses(fake: &Fake, client: m::Id) {
    let mut discovery = value("discovery");
    discovery["value"]["recoveryFormats"] = json!(["formatted-active-buffer-v1", "native0-raw-v1"]);
    fake.json(discovery);
    let mut pair = value("pair-selection");
    pair["value"]["clientId"] = json!(client);
    pair["value"]["recoveryFormat"] = json!("native0-raw-v1");
    pair["value"]["native0Sender"] = json!(codec("2"));
    pair["value"]["native0Receiver"] = json!(codec("1"));
    fake.json(pair);
}
#[test]
fn native0_pair_keeps_actual_selected_sender_separate_from_receiver_offer() {
    let fake = Fake::default();
    let client = ManagerClient::native0(
        AuthenticatedPeerId::new("verified-peer").unwrap(),
        id(2),
        fake.clone(),
        codec("1"),
    )
    .unwrap();
    native_responses(&fake, id(2));
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
    block_on(client.pair(id(3))).unwrap();
    let selected = client.selection().unwrap();
    assert_eq!(selected.native0_sender, Some(codec("2")));
    assert_eq!(selected.native0_receiver, Some(codec("1")));
    let call = &fake.0.lock().unwrap().1[1].body;
    assert_eq!(call["native0Receiver"], json!(codec("1")));
    assert!(call.get("native0Sender").is_none());
}
#[test]
fn same_actual_state_fence_rejects_invalidate_and_rebind_without_callback_or_ack() {
    let fake = Fake::default();
    let client = ManagerClient::native0(
        AuthenticatedPeerId::new("verified-peer").unwrap(),
        id(2),
        fake.clone(),
        codec("1"),
    )
    .unwrap();
    native_responses(&fake, id(2));
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
    block_on(client.pair(id(3))).unwrap();
    let old = client.binding_witness().unwrap();
    let mut commits = 0;
    old.commit_if_current(|| commits += 1).unwrap();
    assert_eq!(commits, 1);
    client.reconnect(id(9));
    assert!(old.commit_if_current(|| commits += 1).is_err());
    assert_eq!(commits, 1);
    native_responses(&fake, id(9));
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
    block_on(client.pair(id(3))).unwrap();
    assert!(old.commit_if_current(|| commits += 1).is_err());
    assert_eq!(commits, 1);
    let before = fake.count();
    let request: m::AcknowledgeTerminal =
        serde_json::from_value(value("request-ack-recovery")["payload"].clone()).unwrap();
    assert!(matches!(
        block_on(client.execute_with_witness(&old, &request)),
        Err(ClientError::StaleConnection)
    ));
    assert_eq!(fake.count(), before);
    let current = client.binding_witness().unwrap();
    current.commit_if_current(|| commits += 1).unwrap();
    assert_eq!(commits, 2);
}
#[test]
fn captured_selection_and_poll_keep_original_cycle_when_binding_ids_are_reused() {
    let fake = Fake::default();
    let client = ManagerClient::native0(
        AuthenticatedPeerId::new("verified-peer").unwrap(),
        id(2),
        fake.clone(),
        codec("1"),
    )
    .unwrap();
    native_responses(&fake, id(2));
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
    block_on(client.pair(id(3))).unwrap();
    let old = client.binding_witness().unwrap();
    client.reconnect(id(2));
    native_responses(&fake, id(2));
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
    block_on(client.pair(id(3))).unwrap();
    let current = client.binding_witness().unwrap();
    assert_eq!(old.binding(), current.binding());
    assert_eq!(old.selection(), current.selection());
    assert_eq!(client.selection().as_ref(), Some(current.selection()));
    assert!(old.commit_if_current(|| ()).is_err());
    current.commit_if_current(|| ()).unwrap();
    let calls = fake.count();
    assert!(matches!(
        block_on(client.poll_with_witness(&old)),
        Err(ClientError::StaleConnection)
    ));
    assert_eq!(fake.count(), calls);
}
#[test]
fn shared_offer_change_keeps_exact_observer_offer_and_selection() {
    use shellfox_client::observer_v2::ObserverClient;
    let fake = Fake::default();
    let mut discovery = value("discovery");
    discovery["value"]["profiles"] = json!(["observer-v2"]);
    fake.json(discovery);
    let offer: Value = serde_json::from_slice(
        &std::fs::read(format!(
            "{}/../shellfox-protocol/tests/fixtures/observer_v2/pair-offer.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    let selected: Value = serde_json::from_slice(
        &std::fs::read(format!(
            "{}/../shellfox-protocol/tests/fixtures/observer_v2/pair-selection.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    fake.json(selected);
    let observer = ObserverClient::new(
        AuthenticatedPeerId::new("verified-peer").unwrap(),
        id(2),
        fake.clone(),
    );
    block_on(observer.discover(FallbackPolicy::Never)).unwrap();
    block_on(observer.pair(id(3))).unwrap();
    assert_eq!(fake.0.lock().unwrap().1[1].body, offer);
}
