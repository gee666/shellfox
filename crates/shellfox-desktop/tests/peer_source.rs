//! Product-source tests without registering an incomplete remote UI in the app.
#[path = "../../native_peer_consumer.rs"]
pub mod consumer;
#[path = "../../shellfox-cli/src/peer_args.rs"]
mod peer_args;
#[path = "../src/peer_source.rs"]
mod peer_source;
#[test]
fn native_event_settlement_capture_preserves_unknown_absence_and_actual_timestamp() {
    use shellfox_protocol::manager_v2 as m;
    let raw: serde_json::Value = serde_json::from_str(include_str!(
        "../../shellfox-protocol/tests/fixtures/observer_v2/snapshot.json"
    ))
    .unwrap();
    let mut snapshot: m::Snapshot = serde_json::from_value(raw["value"].clone()).unwrap();
    let session = snapshot
        .entries
        .iter_mut()
        .find_map(|entry| match entry {
            m::CatalogEntry::Session { value } => Some(value),
            _ => None,
        })
        .unwrap();
    let tab = session.tabs[0].id;
    session.facts.settled_at = m::Fact::Known { value: None };
    assert_eq!(consumer::settlement(&snapshot, tab), Some(None));
    let session = snapshot
        .entries
        .iter_mut()
        .find_map(|entry| match entry {
            m::CatalogEntry::Session { value } => Some(value),
            _ => None,
        })
        .unwrap();
    session.facts.settled_at = m::Fact::Known {
        value: Some("2026-01-01T00:00:00Z".into()),
    };
    assert_eq!(
        consumer::settlement(&snapshot, tab),
        Some(Some("2026-01-01T00:00:00Z".into()))
    );
    let session = snapshot
        .entries
        .iter_mut()
        .find_map(|entry| match entry {
            m::CatalogEntry::Session { value } => Some(value),
            _ => None,
        })
        .unwrap();
    session.facts.settled_at = m::Fact::NotApplicable;
    assert_eq!(consumer::settlement(&snapshot, tab), None);
    assert_eq!(consumer::settlement(&snapshot, m::Id::new()), None);
}
