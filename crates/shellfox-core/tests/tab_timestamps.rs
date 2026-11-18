use serde_json::{json, Value};
use shellfox_core::*;
fn old_tab() -> Value {
    json!({"id":TabId::new(),"sessionId":SessionId::new(),"title":"Shell 1","cwd":"/work","ordinal":0,"profileId":"login-shell","state":"pending"})
}
#[test]
fn missing_historical_tab_creation_remains_unknown_and_serializes_omitted() {
    let tab: Tab = serde_json::from_value(old_tab()).unwrap();
    assert_eq!(tab.created_at, None);
    tab.validate().unwrap();
    assert!(serde_json::to_value(tab)
        .unwrap()
        .get("createdAt")
        .is_none());
}
#[test]
fn known_tab_intent_creation_must_be_a_valid_utc_timestamp() {
    let mut raw = old_tab();
    raw["createdAt"] = json!("2026-07-07T12:00:01.000Z");
    let tab: Tab = serde_json::from_value(raw.clone()).unwrap();
    tab.validate().unwrap();
    assert_eq!(tab.created_at.as_deref(), Some("2026-07-07T12:00:01.000Z"));
    assert_eq!(
        serde_json::to_value(tab).unwrap()["createdAt"],
        raw["createdAt"]
    );
    for value in [
        "",
        "not a time",
        "2026-07-07T12:00:01+02:00",
        "2026-07-07T12:00:01+00:00",
    ] {
        raw["createdAt"] = json!(value);
        assert!(
            serde_json::from_value::<Tab>(raw.clone())
                .unwrap()
                .validate()
                .is_err(),
            "accepted {value}"
        );
    }
    raw["createdAt"] = Value::Null;
    assert_eq!(serde_json::from_value::<Tab>(raw).unwrap().created_at, None);
}
#[test]
fn old_snapshot_does_not_derive_tab_time_from_session_or_add_top_level_fields() {
    let mut raw_tab = old_tab();
    let session_id = SessionId::new();
    raw_tab["sessionId"] = json!(session_id);
    let raw = json!({"revision":1,"sessions":[{"id":session_id,"title":"Work","cwd":"/work","createdAt":"2000-01-01T00:00:00.000Z","updatedAt":"2000-01-01T00:00:00.000Z","settledAt":null,"pinnedAt":null,"env":[],"tabs":[raw_tab]}],"preferences":Preferences::default(),"selection":{"sessionId":null,"tabId":null}});
    let snapshot: Snapshot = serde_json::from_value(raw).unwrap();
    snapshot.sessions[0].validate().unwrap();
    assert_eq!(snapshot.sessions[0].tabs[0].created_at, None);
    let encoded = serde_json::to_value(snapshot).unwrap();
    assert_eq!(encoded.as_object().unwrap().len(), 4);
    assert!(encoded["sessions"][0]["tabs"][0].get("createdAt").is_none());
}
