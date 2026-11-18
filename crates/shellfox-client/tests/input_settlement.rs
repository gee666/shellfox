use serde_json::Value;
use shellfox_client::{protocol::*, *};
const SETTLED: &str = "2026-11-13T12:00:00.000Z";
fn attachment(token: Option<Option<&str>>) -> TerminalAttachment {
    let mut raw: Value = serde_json::from_slice(include_bytes!(
        "fixtures/terminal-attach-owned-success.json"
    ))
    .unwrap();
    let value = raw["value"].as_object_mut().unwrap();
    match token {
        None => {
            value.remove("settledAt");
        }
        Some(None) => {
            value.insert("settledAt".into(), Value::Null);
        }
        Some(Some(value)) => {
            raw["value"]["settledAt"] = value.into();
        }
    }
    let Reply::Success(a) =
        decode::<Reply<TerminalAttachment>>(&serde_json::to_vec(&raw).unwrap()).unwrap()
    else {
        panic!()
    };
    a
}
fn event(a: &TerminalAttachment, token: Option<&str>) -> TerminalEvent {
    let TerminalAttachment::Owned(a) = a else {
        panic!()
    };
    TerminalEvent::Settlement {
        tab_id: a.tab_id,
        generation: a.generation,
        settled_at: token.map(str::to_owned),
    }
}
#[test]
fn attachment_settlement_preserves_absent_null_and_timestamp_on_writes() {
    for token in [None, Some(None), Some(Some(SETTLED))] {
        let a = attachment(token);
        let cursor = ViewCursor::from_attachment(&a, 1).unwrap();
        let write = cursor.write("input".into(), Some(true), 1).unwrap();
        assert_eq!(write.settled_at, token.map(|v| v.map(str::to_owned)));
        let encoded = serde_json::to_value(write).unwrap();
        match token {
            None => assert!(encoded.get("settledAt").is_none()),
            Some(None) => assert!(encoded["settledAt"].is_null()),
            Some(Some(token)) => assert_eq!(encoded["settledAt"], token),
        }
    }
}
#[test]
fn settlement_changes_only_matching_generation_and_queued_input_keeps_admission_token() {
    let a = attachment(Some(None));
    let mut cursor = ViewCursor::from_attachment(&a, 1).unwrap();
    let queued = cursor
        .write("queued before settle".into(), Some(true), 1)
        .unwrap();
    assert_eq!(queued.settled_at, Some(None));
    let mut wrong = event(&a, Some(SETTLED));
    if let TerminalEvent::Settlement { generation, .. } = &mut wrong {
        *generation = "60000000-0000-4000-8000-000000000001".parse().unwrap();
    }
    assert_eq!(cursor.observe(&wrong, 1).unwrap(), EventDisposition::Ignore);
    assert_eq!(
        cursor
            .write("still null".into(), None, 1)
            .unwrap()
            .settled_at,
        Some(None)
    );
    assert_eq!(
        cursor.observe(&event(&a, Some(SETTLED)), 1).unwrap(),
        EventDisposition::Deliver
    );
    let fresh = cursor.write("new input".into(), None, 1).unwrap();
    assert_eq!(queued.settled_at, Some(None));
    assert_eq!(fresh.settled_at, Some(Some(SETTLED.into())));
    cursor.observe(&event(&a, None), 1).unwrap();
    assert_eq!(
        cursor
            .write("after clear".into(), None, 1)
            .unwrap()
            .settled_at,
        Some(None)
    );
    assert_eq!(fresh.settled_at, Some(Some(SETTLED.into())));
}
#[test]
fn binding_can_reapply_new_matching_settlement_after_older_attach_reply() {
    let older = attachment(Some(None));
    // Binding records this event after its attach admission marker, before the reply.
    let newer = event(&older, Some(SETTLED));
    let mut cursor = ViewCursor::from_attachment(&older, 1).unwrap();
    cursor.observe(&newer, 1).unwrap();
    assert_eq!(
        cursor
            .write("new input".into(), Some(true), 1)
            .unwrap()
            .settled_at,
        Some(Some(SETTLED.into()))
    );
}
#[test]
fn ctrl_space_nul_is_terminal_input_not_permission_to_put_nul_in_paths_or_env() {
    let a = attachment(None);
    let cursor = ViewCursor::from_attachment(&a, 1).unwrap();
    let write = cursor.write("\0".into(), Some(true), 1).unwrap();
    write.validate_wire().unwrap();
    assert_eq!(serde_json::to_value(&write).unwrap()["data"], "\0");
    assert_eq!(write.data.as_bytes(), &[0]);
    for path in ["/tmp/\0name", "name\0"] {
        assert!(files::FileLocation {
            session_id: None,
            tab_id: None,
            path: path.into()
        }
        .validate()
        .is_err());
    }
    assert!(SetTerminalEnv {
        env: vec![EnvVar {
            name: "A".into(),
            value: "\0".into()
        }]
    }
    .validate()
    .is_err());
    assert!(ResolveDirectory { path: "\0".into() }.validate().is_err());
}
