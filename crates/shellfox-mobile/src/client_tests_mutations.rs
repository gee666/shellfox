use super::*;
use crate::client::mutations::{MAX_BYTES, MAX_INTENTS};
fn write(h: &Harness, view: &NativeView, bytes: &[u8]) -> (i32, u64) {
    let mut operation = 99;
    let status = unsafe {
        sfm_client_terminal_write(
            h.handle,
            view.0,
            bytes.as_ptr(),
            bytes.len(),
            &mut operation,
        )
    };
    (status, operation)
}
fn write_hint(h: &Harness, view: &NativeView, bytes: &[u8], hint: u8) -> (i32, u64) {
    let mut operation = 99;
    let status = unsafe {
        sfm_client_terminal_write_ex(
            h.handle,
            view.0,
            bytes.as_ptr(),
            bytes.len(),
            hint,
            &mut operation,
        )
    };
    (status, operation)
}
fn resize(h: &Harness, view: &NativeView) -> (i32, u64) {
    let mut operation = 99;
    let status = unsafe { sfm_client_terminal_resize(h.handle, view.0, 90, 30, &mut operation) };
    (status, operation)
}
fn written(h: &Harness, request: &NativeRequest) {
    assert_eq!(
        h.complete(request, br#"{"ok":true,"value":{"written":true}}"#),
        0
    );
}
#[test]
fn synchronous_admission_preserves_write_resize_write_order_while_first_go_call_is_held() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    let first_id = write(&h, &view, b"key-down");
    assert_eq!(first_id.0, 0);
    let first = h.request();
    assert_eq!(first.body["payload"]["data"], "key-down");
    let middle_id = resize(&h, &view);
    assert_eq!(middle_id.0, 0);
    let release_id = write(&h, &view, b"\x1b[97;1:3u");
    assert_eq!(release_id.0, 0);
    h.dispatch();
    assert!(
        h.mock.requests.is_empty(),
        "no future poll order may overtake the held write"
    );
    assert_eq!(
        registry_lookup(h.handle)
            .unwrap()
            .state
            .lock()
            .unwrap()
            .requests
            .len(),
        1
    );
    written(&h, &first);
    let middle = h.request();
    assert_eq!(middle.body["method"], "resizeTerminal");
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    assert_eq!(
        h.complete(&middle, br#"{"ok":true,"value":{"resized":true}}"#),
        0
    );
    let release = h.request();
    assert_eq!(release.body["method"], "writeTerminal");
    assert_eq!(release.body["payload"]["data"], "\x1b[97;1:3u");
    written(&h, &release);
    let mut ids = Vec::new();
    for _ in 0..3 {
        let event = h.event();
        assert_eq!(event["internal"], true);
        assert_eq!(event["status"], 0);
        ids.push(event["operationId"].as_str().unwrap().to_owned());
    }
    for id in [first_id.1, middle_id.1, release_id.1] {
        assert!(ids.contains(&id.to_string()));
    }
}
#[test]
fn mutation_bytes_busy_accepts_no_id_and_release_retry_keeps_existing_order() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    let bytes = vec![b'x'; 65536];
    assert_eq!(write(&h, &view, &bytes).0, 0);
    let first = h.request();
    for _ in 0..MAX_BYTES / bytes.len() - 1 {
        assert_eq!(write(&h, &view, &bytes).0, 0);
    }
    assert_eq!(write(&h, &view, b"release"), (BUSY, 0));
    written(&h, &first);
    assert_eq!(h.event()["status"], 0);
    let second = h.request();
    assert_eq!(
        second.body["payload"]["data"].as_str().unwrap().len(),
        65536
    );
    assert_eq!(write(&h, &view, b"release").0, 0);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    written(&h, &second);
    for _ in 0..MAX_BYTES / bytes.len() - 2 {
        h.event();
        let next = h.request();
        written(&h, &next);
    }
    let release = h.request();
    assert_eq!(release.body["payload"]["data"], "release");
    written(&h, &release);
    h.event();
    h.event();
}
#[test]
fn ten_thousand_keys_typed_while_a_write_is_held_go_out_joined_in_order() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(write(&h, &view, b"first").0, 0);
    let first = h.request();
    let typed: String = "ls -la; echo fox\r".chars().cycle().take(10_000).collect();
    let mut ids = std::collections::HashSet::new();
    for c in typed.chars() {
        let (status, id) = write(&h, &view, c.to_string().as_bytes());
        assert_eq!(status, 0);
        ids.insert(id);
    }
    assert_eq!(ids.len(), 1, "every key joined the one waiting write");
    written(&h, &first);
    let joined = h.request();
    assert_eq!(joined.body["payload"]["data"], typed.as_str());
    written(&h, &joined);
    h.event();
    h.event();
}
#[test]
fn mutation_count_bound_does_not_block_ack_or_poll_behind_input_lane() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    // Writes with different hints never join, so each takes a slot.
    for n in 0..MAX_INTENTS {
        assert_eq!(write_hint(&h, &view, b"x", ((n + 1) % 2) as u8).0, 0);
    }
    assert_eq!(write(&h, &view, b"release"), (BUSY, 0));
    let first = h.request();
    assert_eq!(first.body["method"], "writeTerminal");
    crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
    assert_eq!(sfm_client_terminal_consumed(h.handle, view.0), 0);
    let ack = h.request();
    assert_eq!(ack.body["method"], "acknowledgeTerminal");
    let mut poll_id = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut poll_id) }, 0);
    let poll = h.request();
    assert_eq!(poll.path, "/v1/events");
    assert_eq!(
        h.complete(&ack, br#"{"ok":true,"value":{"acknowledged":true}}"#),
        0
    );
    assert_eq!(
        h.complete(&poll, br#"{"ok":true,"value":{"events":[]}}"#),
        0
    );
    h.event();
    h.event();
    // Closing the client fences all 64 admitted intents, not just the sent one.
    h.close();
}
#[test]
fn control_revocation_fences_active_and_queued_mutations_before_any_more_dispatch() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(write(&h, &view, b"down").0, 0);
    let first = h.request();
    assert_eq!(resize(&h, &view).0, 0);
    assert_eq!(write(&h, &view, b"up").0, 0);
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut op) }, 0);
    let poll = h.request();
    let blocked =
        include_bytes!("../../shellfox-client/tests/fixtures/terminal-control-event-success.json");
    assert_eq!(h.complete(&poll, blocked), 0);
    for _ in 0..4 {
        let event = h.event();
        assert_eq!(event["internal"], true);
    }
    assert_eq!(
        h.complete(&first, br#"{"ok":true,"value":{"written":true}}"#),
        STALE
    );
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    assert_eq!(write(&h, &view, b"stale"), (STALE, 0));
}
fn reattach(h: &mut Harness, view: &NativeView) {
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        0
    );
    let attach = h.request();
    let response =
        include_bytes!("../../shellfox-client/tests/fixtures/terminal-attach-owned-success.json");
    assert_eq!(h.complete(&attach, response), 0);
    let event = h.event();
    reset_for_event(h, view.0, &event);
    crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
}
#[test]
fn a_write_the_host_refuses_drops_only_that_input() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(write(&h, &view, b"too fast").0, 0);
    let first = h.request();
    assert_eq!(h.complete(&first, br#"{"ok":false,"error":{"code":"VALIDATION","message":"Terminal input rate exceeded","retryable":false}}"#), 0);
    let event = h.event();
    assert_eq!(event["result"]["ok"], false);
    assert!(event.get("reattach").is_none());
    assert_eq!(write(&h, &view, b"next").0, 0);
    assert_eq!(h.request().body["payload"]["data"], "next");
}
#[test]
fn lost_control_asks_js_to_reattach_and_input_resumes_after_it() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(write(&h, &view, b"down").0, 0);
    let first = h.request();
    assert_eq!(h.complete(&first, br#"{"ok":false,"error":{"code":"TERMINAL_CONTROL_REQUIRED","message":"Take control first.","retryable":false}}"#), 0);
    let event = h.event();
    assert_eq!(event["reattach"][0]["tabId"], TAB);
    assert_eq!(write(&h, &view, b"before-reattach"), (STALE, 0));
    // The reattach failed: the next keystroke asks again.
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        0
    );
    let failed = h.request();
    assert_eq!(h.complete(&failed, br#"{"ok":false,"error":{"code":"NATIVE_UNAVAILABLE","message":"busy","retryable":true}}"#), 0);
    assert_eq!(h.event()["result"]["ok"], false);
    assert_eq!(write(&h, &view, b"after-failed-attach"), (STALE, 0));
    assert_eq!(h.event()["reattach"][0]["tabId"], TAB);
    reattach(&mut h, &view);
    assert_eq!(write(&h, &view, b"after").0, 0);
    assert_eq!(h.request().body["payload"]["data"], "after");
}
fn refuse_resize(h: &mut Harness, request: &NativeRequest, code: &str) {
    let reply = json!({"ok":false,"error":{"code":code,"message":"refused","retryable":false}});
    assert_eq!(h.complete(request, reply.to_string().as_bytes()), 0);
}
#[test]
fn a_refused_resize_keeps_typing_and_is_asked_again_a_few_times() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(resize(&h, &view).0, 0);
    let first = h.request();
    refuse_resize(&mut h, &first, "NATIVE_UNAVAILABLE");
    assert!(h.event().get("reattach").is_none());
    assert_eq!(write(&h, &view, b"typed").0, 0);
    let typed = h.request();
    assert_eq!(typed.body["payload"]["data"], "typed");
    written(&h, &typed);
    // Asked again after a second, then at most three times in all.
    for _ in 0..3 {
        let retry = h.request();
        assert_eq!(retry.body["method"], "resizeTerminal");
        assert_eq!(retry.body["payload"]["rows"], 30);
        refuse_resize(&mut h, &retry, "NATIVE_UNAVAILABLE");
    }
    thread::sleep(Duration::from_millis(1500));
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn a_newer_size_cancels_the_retry_of_a_refused_one() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(resize(&h, &view).0, 0);
    let first = h.request();
    refuse_resize(&mut h, &first, "NATIVE_UNAVAILABLE");
    let mut operation = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_resize(h.handle, view.0, 100, 40, &mut operation) },
        0
    );
    let newer = h.request();
    assert_eq!(newer.body["payload"]["rows"], 40);
    assert_eq!(
        h.complete(&newer, br#"{"ok":true,"value":{"resized":true}}"#),
        0
    );
    thread::sleep(Duration::from_millis(1500));
    h.dispatch();
    assert!(
        h.mock.requests.is_empty(),
        "the old size came back: {:?}",
        h.mock.requests
    );
}
#[test]
fn a_host_that_always_loses_resizes_reattaches_only_once() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(resize(&h, &view).0, 0);
    let first = h.request();
    refuse_resize(&mut h, &first, "TARGET_LOST");
    assert_eq!(h.event()["reattach"][0]["tabId"], TAB);
    reattach(&mut h, &view);
    assert_eq!(resize(&h, &view).0, 0);
    let again = h.request();
    refuse_resize(&mut h, &again, "TARGET_LOST");
    let event = h.event();
    assert!(event.get("reattach").is_none(), "no reattach loop: {event}");
    assert_eq!(write(&h, &view, b"still typing").0, 0);
    assert_eq!(h.request().body["payload"]["data"], "still typing");
}
#[test]
fn uncertain_mutation_is_never_retried_and_cancels_successors() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    assert_eq!(write(&h, &view, b"down").0, 0);
    let first = h.request();
    assert_eq!(resize(&h, &view).0, 0);
    assert_eq!(write(&h, &view, b"up").0, 0);
    assert_eq!(
        unsafe { sfm_client_complete(h.handle, first.generation, first.id, 3, ptr::null(), 0) },
        0
    );
    let mut failed = false;
    for _ in 0..3 {
        let event = h.event();
        assert_eq!(event["internal"], true);
        assert_ne!(event["status"], 0);
        failed |= event["status"] == NETWORK;
    }
    assert!(failed);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    assert_eq!(write(&h, &view, b"retry-not-allowed"), (STALE, 0));
}
