use super::*;
const EARLIER: &str = "2026-01-01T00:00:00.000Z";
const LATER: &str = "2026-01-01T00:00:01.000Z";
fn binding(h: &Harness) -> NativeView {
    let view = NativeView(crate::engine::create(80, 24).unwrap());
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), view.0) },
        0
    );
    view
}
fn attachment(h: &Harness) {
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        0
    );
}
fn response(token: Option<Value>) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../shellfox-client/tests/fixtures/terminal-attach-owned-success.json"
    ))
    .unwrap();
    value["value"].as_object_mut().unwrap().remove("settledAt");
    if let Some(token) = token {
        value["value"]["settledAt"] = token;
    }
    serde_json::to_vec(&value).unwrap()
}
fn write(h: &Harness, view: &NativeView, data: &[u8]) -> u64 {
    let mut operation = 0;
    assert_eq!(
        unsafe {
            sfm_client_terminal_write(h.handle, view.0, data.as_ptr(), data.len(), &mut operation)
        },
        0
    );
    operation
}
fn written(h: &Harness, request: &NativeRequest) {
    assert_eq!(
        h.complete(request, br#"{"ok":true,"value":{"written":true}}"#),
        0
    );
}
fn settlement(token: Value, generation: &str) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"ok":true,"value":{"events":[{"type":"terminal","event":{
        "type":"settlement","tabId":TAB,"generation":generation,"settledAt":token}}]}}),
    )
    .unwrap()
}
#[test]
fn attachment_and_native_confirmation_preserve_absent_null_and_timestamp() {
    for token in [None, Some(Value::Null), Some(json!(EARLIER))] {
        let mut h = Harness::new();
        h.connect();
        let view = binding(&h);
        attachment(&h);
        let request = h.request();
        assert_eq!(h.complete(&request, &response(token.clone())), 0);
        let event = h.event();
        assert_eq!(event["result"]["value"].get("settledAt"), token.as_ref());
        reset_for_event(&h, view.0, &event);
        crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
        write(&h, &view, b"x");
        let request = h.request();
        assert_eq!(request.body["payload"].get("settledAt"), token.as_ref());
        written(&h, &request);
        h.event();
    }
}
#[test]
fn settlement_during_attach_or_held_reset_is_applied_before_native_input() {
    for token in [Value::Null, json!(LATER)] {
        for before_response in [true, false] {
            let mut h = Harness::new();
            h.connect();
            let view = binding(&h);
            let mut operation = 0;
            assert_eq!(unsafe { sfm_client_poll(h.handle, &mut operation) }, 0);
            let poll = h.request();
            attachment(&h);
            let attach = h.request();
            let live = settlement(token.clone(), "30000000-0000-4000-8000-000000000001");
            if before_response {
                assert_eq!(h.complete(&poll, &live), 0);
                h.event();
            }
            assert_eq!(h.complete(&attach, &response(Some(json!(EARLIER)))), 0);
            let event = h.event();
            if !before_response {
                assert_eq!(h.complete(&poll, &live), 0);
                h.event();
            }
            reset_for_event(&h, view.0, &event);
            crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
            write(&h, &view, b"x");
            let request = h.request();
            assert_eq!(request.body["payload"].get("settledAt"), Some(&token));
            written(&h, &request);
            h.event();
        }
    }
}
#[test]
fn stale_generation_during_attach_cannot_replace_matching_settlement() {
    let mut h = Harness::new();
    h.connect();
    let view = binding(&h);
    let mut operation = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut operation) }, 0);
    let poll = h.request();
    attachment(&h);
    let attach = h.request();
    let mut live: Value = serde_json::from_slice(&settlement(
        json!(LATER),
        "30000000-0000-4000-8000-000000000001",
    ))
    .unwrap();
    let stale: Value = serde_json::from_slice(&settlement(
        Value::Null,
        "30000000-0000-4000-8000-000000000002",
    ))
    .unwrap();
    live["value"]["events"]
        .as_array_mut()
        .unwrap()
        .push(stale["value"]["events"][0].clone());
    assert_eq!(h.complete(&poll, &serde_json::to_vec(&live).unwrap()), 0);
    h.event();
    assert_eq!(h.complete(&attach, &response(Some(json!(EARLIER)))), 0);
    let event = h.event();
    reset_for_event(&h, view.0, &event);
    crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
    write(&h, &view, b"x");
    let request = h.request();
    assert_eq!(request.body["payload"]["settledAt"], LATER);
    written(&h, &request);
    h.event();
}
#[test]
fn queued_input_keeps_admission_token_after_a_new_manual_settlement() {
    for initial in [Value::Null, json!(EARLIER)] {
        let mut h = Harness::new();
        h.connect();
        let view = binding(&h);
        attachment(&h);
        let attach = h.request();
        assert_eq!(h.complete(&attach, &response(Some(initial.clone()))), 0);
        let event = h.event();
        reset_for_event(&h, view.0, &event);
        crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
        write(&h, &view, b"first");
        let first = h.request();
        write(&h, &view, b"queued-before-new-settle");
        let mut operation = 0;
        assert_eq!(unsafe { sfm_client_poll(h.handle, &mut operation) }, 0);
        let poll = h.request();
        assert_eq!(
            h.complete(
                &poll,
                &settlement(json!(LATER), "30000000-0000-4000-8000-000000000001")
            ),
            0
        );
        h.event();
        write(&h, &view, b"queued-after-new-settle");
        assert_eq!(first.body["payload"].get("settledAt"), Some(&initial));
        written(&h, &first);
        let second = h.request();
        assert_eq!(second.body["payload"].get("settledAt"), Some(&initial));
        written(&h, &second);
        let third = h.request();
        assert_eq!(third.body["payload"]["settledAt"], LATER);
        written(&h, &third);
        for _ in 0..3 {
            h.event();
        }
    }
}
