use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use shellfox_client::protocol::{
    decode, Endpoint, ErrorCode, ManagerSnapshot, PairResponse, Permission, PollResponse, Reply,
    TerminalAttachment, Validate, MAX_RESPONSE_BYTES, MAX_SAFE_INTEGER,
};

const SNAPSHOT: &str = include_str!("fixtures/manager-snapshot-success.json");
const OWNED: &str = include_str!("fixtures/terminal-attach-owned-success.json");
const RECOVERY: &str = include_str!("fixtures/terminal-attach-recovery-success.json");
const BLOCKED: &str = include_str!("fixtures/terminal-attach-blocked-success.json");
const EVENTS: &str = include_str!("fixtures/terminal-events-success.json");
const CONTROL: &str = include_str!("fixtures/terminal-control-event-success.json");
const PAIR: &str = include_str!("fixtures/remote-pair-success.json");
const AUTH: &str = include_str!("fixtures/remote-auth-failed.json");
const APPROVAL: &str = include_str!("fixtures/remote-approval-required.json");
const DENIED: &str = include_str!("fixtures/remote-access-denied.json");

fn parse<T: DeserializeOwned + Validate>(source: &str) -> T {
    decode(source.as_bytes()).expect("fixture must decode and validate")
}

fn roundtrip<T: DeserializeOwned + Serialize + Validate>(source: &str) {
    let original: Value = serde_json::from_str(source).unwrap();
    let decoded: T = parse(source);
    assert_eq!(serde_json::to_value(decoded).unwrap(), original);
}

fn rejects<T: DeserializeOwned + Validate>(value: &Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    assert!(
        decode::<T>(&bytes).is_err(),
        "accepted invalid message: {value}"
    );
}

fn value(source: &str) -> Value {
    serde_json::from_str(source).unwrap()
}

#[test]
fn snapshot_fixture_decodes_and_validates() {
    let snapshot: Reply<ManagerSnapshot> = parse(SNAPSHOT);
    assert!(matches!(snapshot, Reply::Success(_)));
}

#[test]
fn snapshot_fixture_roundtrips_nullable_fields() {
    // Zod defaults pythonPath to null. Hours are JSON numbers, represented as f64 in Rust.
    let mut expected = value(SNAPSHOT);
    expected["value"]["settings"]["pythonPath"] = Value::Null;
    expected["value"]["settings"]["sessionSettleHours"] = json!(24.0);
    expected["value"]["settings"]["sessionDeleteHours"] = json!(720.0);
    let decoded: Reply<ManagerSnapshot> = parse(SNAPSHOT);
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn attachment_and_poll_fixtures_roundtrip() {
    for source in [OWNED, RECOVERY, BLOCKED] {
        roundtrip::<Reply<TerminalAttachment>>(source);
    }
    for source in [EVENTS, CONTROL] {
        roundtrip::<Reply<PollResponse>>(source);
    }
    let Reply::Success(TerminalAttachment::Owned(recovery)) = parse(RECOVERY) else {
        panic!("expected owned recovery");
    };
    assert_eq!(recovery.snapshot, Some(true));
    assert!(recovery.truncated);
    assert_eq!(recovery.chunks[0].data, "\x1b[H\x1b[2Jprompt$ ");
    assert!(matches!(
        parse::<Reply<TerminalAttachment>>(BLOCKED),
        Reply::Success(TerminalAttachment::Blocked { .. })
    ));
}

#[test]
fn pair_and_permission_failures_roundtrip() {
    roundtrip::<Reply<PairResponse>>(PAIR);
    for (source, code, permission) in [
        (AUTH, ErrorCode::AuthFailed, Permission::Unknown),
        (
            APPROVAL,
            ErrorCode::ApprovalRequired,
            Permission::AwaitingApproval,
        ),
        (DENIED, ErrorCode::AccessDenied, Permission::Rejected),
    ] {
        roundtrip::<Reply<PairResponse>>(source);
        let Reply::Failure(error) = parse::<Reply<PairResponse>>(source) else {
            panic!("expected failure")
        };
        assert_eq!(error.code, code);
        assert_eq!(Permission::from_error(&error), permission);
    }
    assert_eq!(Endpoint::Discover.path(), "/v1/discover");
    assert_eq!(Endpoint::Pair.path(), "/v1/pair");
    assert_eq!(Endpoint::Request.path(), "/v1/request");
    assert_eq!(Endpoint::Events.path(), "/v1/events");
    assert_eq!(Endpoint::Disconnect.path(), "/v1/disconnect");
}

#[test]
fn unknown_fields_and_blocked_content_are_rejected() {
    let mut input = value(SNAPSHOT);
    input["value"]["sessions"][0]["unknown"] = json!(true);
    rejects::<Reply<ManagerSnapshot>>(&input);
    let mut input = value(BLOCKED);
    input["value"]["chunks"] = json!([]);
    rejects::<Reply<TerminalAttachment>>(&input);
    let mut input = value(EVENTS);
    input["value"]["events"][0]["event"]["extra"] = json!(1);
    rejects::<Reply<PollResponse>>(&input);
    let mut input = value(PAIR);
    input["value"]["terminal_control"] = json!("exclusive-v1");
    rejects::<Reply<PairResponse>>(&input);
}

#[test]
fn invalid_reply_discriminators_and_branches_are_rejected() {
    for input in [
        json!({"ok": "true", "value": {"paired": true, "terminalControl": "exclusive-v1"}}),
        json!({"ok": true}),
        json!({"ok": false, "value": {"paired": true, "terminalControl": "exclusive-v1"}}),
        json!({"ok": true, "value": {"paired": true, "terminalControl": "exclusive-v1"}, "error": {"code": "AUTH_FAILED", "message": "no", "retryable": false}}),
        json!({"ok": false, "error": {"code": "AUTH_FAILED", "message": "no", "retryable": false}, "value": {"paired": true, "terminalControl": "exclusive-v1"}}),
    ] {
        rejects::<Reply<PairResponse>>(&input);
    }
}

#[test]
fn success_reply_rejects_null_error_field() {
    let mut success = value(PAIR);
    success["error"] = Value::Null;
    rejects::<Reply<PairResponse>>(&success);
}

#[test]
fn failure_reply_rejects_null_value_field() {
    let mut failure = value(AUTH);
    failure["value"] = Value::Null;
    rejects::<Reply<PairResponse>>(&failure);
}

#[test]
fn optional_nullable_fields_preserve_null_and_omission() {
    for source in [OWNED, RECOVERY] {
        let mut present = value(source);
        present["value"]["settledAt"] = Value::Null;
        let decoded: Reply<TerminalAttachment> =
            decode(&serde_json::to_vec(&present).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), present);
        let mut absent = value(source);
        absent["value"].as_object_mut().unwrap().remove("settledAt");
        let decoded: Reply<TerminalAttachment> =
            decode(&serde_json::to_vec(&absent).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), absent);
    }
}

#[test]
fn invalid_attachment_bounds_and_chunk_identity_are_rejected() {
    let cases = [
        ("controlEpoch", json!(0)),
        ("controlEpoch", json!(MAX_SAFE_INTEGER + 1)),
        ("firstSequence", json!(0)),
        ("lastSequence", json!(MAX_SAFE_INTEGER + 1)),
        ("firstSequence", json!(3)),
        ("cols", json!(1)),
        ("rows", json!(501)),
        ("lifetime", json!("external-legacy")),
    ];
    for (field, bad) in cases {
        let mut input = value(OWNED);
        input["value"][field] = bad;
        rejects::<Reply<TerminalAttachment>>(&input);
    }
    for (field, bad) in [
        ("sequence", json!(0)),
        ("sequence", json!(2)),
        ("tabId", json!("20000000-0000-4000-8000-000000000002")),
        ("generation", json!("30000000-0000-4000-8000-000000000002")),
        ("data", json!("")),
        ("data", json!("x".repeat(16385))),
    ] {
        let mut input = value(OWNED);
        input["value"]["chunks"][0][field] = bad;
        rejects::<Reply<TerminalAttachment>>(&input);
    }
    let mut input = value(OWNED);
    let chunk = input["value"]["chunks"][0].clone();
    input["value"]["chunks"].as_array_mut().unwrap().push(chunk);
    rejects::<Reply<TerminalAttachment>>(&input);
}

#[test]
fn invalid_events_pair_and_runtime_size_are_rejected() {
    for (field, bad) in [
        ("controlEpoch", json!(0)),
        ("sequence", json!(0)),
        ("sequence", json!(MAX_SAFE_INTEGER + 1)),
        ("data", json!("x".repeat(16385))),
    ] {
        let mut input = value(EVENTS);
        input["value"]["events"][0]["event"][field] = bad;
        rejects::<Reply<PollResponse>>(&input);
    }
    let mut input = value(CONTROL);
    input["value"]["events"][0]["event"]["controlEpoch"] = json!(0);
    rejects::<Reply<PollResponse>>(&input);
    let mut input = value(EVENTS);
    let event = input["value"]["events"][0].clone();
    input["value"]["events"] = json!(vec![event; 1025]);
    rejects::<Reply<PollResponse>>(&input);
    for (field, bad) in [
        ("paired", json!(false)),
        ("terminalControl", json!("exclusive-v2")),
    ] {
        let mut input = value(PAIR);
        input["value"][field] = bad;
        rejects::<Reply<PairResponse>>(&input);
    }
    assert!(decode::<Reply<PairResponse>>(&vec![b' '; MAX_RESPONSE_BYTES + 1]).is_err());
}
