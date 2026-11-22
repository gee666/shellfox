use super::*;

#[test]
fn mock_native_go_pairs_and_executes_real_typed_method() {
    let mut h = Harness::new();
    h.connect();
    let (status, op) = h.operation(json!({"method":"getHomeDirectory","payload":{}}));
    assert_eq!(status, 0);
    let req = h.request();
    assert_eq!(req.path, "/v1/request");
    assert_eq!(req.body["method"], "getHomeDirectory");
    assert_eq!(
        h.complete(&req, br#"{"ok":true,"value":{"cwd":"/fake-home"}}"#),
        0
    );
    let event = h.event();
    assert_eq!(event["operationId"], op.to_string());
    assert_eq!(event["result"]["value"]["cwd"], "/fake-home");
    assert_eq!(h.complete(&req, PAIR), STALE);
}
#[test]
fn graceful_disconnect_uses_real_shared_view_withdrawal() {
    let mut h = Harness::new();
    h.connect();
    let mut operation = 0;
    assert_eq!(
        unsafe { sfm_client_disconnect(h.handle, &mut operation) },
        0
    );
    let req = h.request();
    assert_eq!(req.path, "/v1/disconnect");
    assert_eq!(req.body["clientId"], "10000000-0000-4000-8000-000000000001");
    assert_eq!(
        h.complete(&req, br#"{"ok":true,"value":{"disconnected":true}}"#),
        0
    );
    assert_eq!(h.event()["connection"]["status"], "disconnected");
    h.close();
}
#[test]
fn rejects_unknown_local_methods_bad_schema_and_oversize_before_go() {
    let mut h = Harness::new();
    for value in [
        json!({"method":"runLocalHelper","payload":{}}),
        json!({"method":"getSnapshot","payload":{"extra":true}}),
        json!({"method":"resizeTerminal","payload":{}}),
    ] {
        assert_eq!(h.operation(value), (INVALID, 0));
    }
    let bytes = vec![b' '; shellfox_protocol::MAX_REQUEST_BYTES + 1];
    let mut op = 2;
    assert_eq!(
        unsafe { sfm_client_operation(h.handle, bytes.as_ptr(), bytes.len(), &mut op) },
        INVALID
    );
    assert_eq!(op, 0);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn approval_and_revocation_are_host_decisions() {
    let mut h = Harness::new();
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_connect(h.handle, &mut op) }, 0);
    let req = h.request();
    let approval =
        include_bytes!("../../shellfox-client/tests/fixtures/remote-approval-required.json");
    assert_eq!(h.complete(&req, approval), 0);
    assert_eq!(h.event()["connection"]["permission"], "awaitingApproval");
    assert_eq!(unsafe { sfm_client_connect(h.handle, &mut op) }, 0);
    let req = h.request();
    assert_eq!(h.complete(&req, PAIR), 0);
    h.event();
    assert_eq!(
        h.operation(json!({"method":"getSnapshot","payload":{}})).0,
        0
    );
    let req = h.request();
    let denied = include_bytes!("../../shellfox-client/tests/fixtures/remote-access-denied.json");
    assert_eq!(h.complete(&req, denied), 0);
    assert_eq!(h.event()["connection"]["permission"], "rejected");
}
#[test]
fn close_cancels_dispatched_go_and_rejects_late_completion() {
    let mut h = Harness::new();
    h.connect();
    assert_eq!(
        h.operation(json!({"method":"getSnapshot","payload":{}})).0,
        0
    );
    let req = h.request();
    let handle = h.handle;
    h.close();
    assert!(h.mock.cancelled.contains(&(req.generation, req.id)));
    assert_eq!(
        unsafe {
            sfm_client_complete(handle, req.generation, req.id, 0, PAIR.as_ptr(), PAIR.len())
        },
        STALE
    );
    // No request/context pointer is retained by Rust after close returns.
}
#[test]
fn cancelled_read_unwinds_without_losing_connection_or_replaying() {
    let mut h = Harness::new();
    h.connect();
    let (status, op) = h.operation(json!({"method":"getHomeDirectory","payload":{}}));
    assert_eq!(status, 0);
    let req = h.request();
    assert_eq!(sfm_client_cancel(h.handle, op), 0);
    let cancelled = h.event();
    assert_eq!(cancelled["status"], CANCELLED);
    assert_eq!(cancelled["connection"]["status"], "connected");
    h.dispatch();
    assert!(h.mock.cancelled.contains(&(req.generation, req.id)));
    assert_eq!(h.complete(&req, PAIR), STALE);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    assert_eq!(
        h.operation(json!({"method":"getHomeDirectory","payload":{}}))
            .0,
        0
    );
    let next = h.request();
    assert_ne!(next.id, req.id);
    assert_eq!(
        h.complete(&next, br#"{"ok":true,"value":{"cwd":"/fake-home"}}"#),
        0
    );
    assert_eq!(h.event()["status"], 0);
}
#[test]
fn malformed_and_oversized_responses_are_not_success() {
    let mut h = Harness::new();
    h.connect();
    assert_eq!(
        h.operation(json!({"method":"getHomeDirectory","payload":{}}))
            .0,
        0
    );
    let req = h.request();
    let huge = vec![b' '; shellfox_protocol::MAX_RESPONSE_BYTES + 1];
    assert_eq!(h.complete(&req, &huge), INVALID);
    assert_eq!(
        h.complete(&req, br#"{"ok":true,"value":{"cwd":"relative"}}"#),
        0
    );
    assert_eq!(h.event()["status"], PROTOCOL);
}
#[test]
fn reconnect_fences_async_native_tokens_and_uses_fresh_identity() {
    let mut h = Harness::new();
    h.connect();
    assert_eq!(
        h.operation(json!({"method":"getHomeDirectory","payload":{}}))
            .0,
        0
    );
    let old = h.request();
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_reconnect(h.handle, &mut op) }, 0);
    assert_eq!(h.complete(&old, PAIR), STALE);
    let pair = h.request();
    assert!(pair.generation > old.generation);
    assert_ne!(pair.body["clientId"], old.body["clientId"]);
    assert_eq!(h.complete(&pair, PAIR), 0);
    let mut found = false;
    let operation_id = op.to_string();
    for _ in 0..2 {
        let e = h.event();
        if e["operationId"].as_str() == Some(operation_id.as_str()) {
            assert_eq!(e["status"], 0);
            found = true;
        }
    }
    assert!(found);
}
#[test]
fn waker_fires_for_queued_requests_and_results_until_close() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    unsafe extern "C" fn wake(context: *mut c_void, _: u64) {
        unsafe { &*context.cast::<AtomicUsize>() }.fetch_add(1, Ordering::SeqCst);
    }
    fn settle(wakes: &AtomicUsize, at_least: usize) {
        let start = Instant::now();
        while wakes.load(Ordering::SeqCst) < at_least {
            assert!(start.elapsed() < Duration::from_secs(5), "no wake");
            std::thread::yield_now();
        }
    }
    let mut h = Harness::new();
    let wakes = Box::leak(Box::new(AtomicUsize::new(0)));
    let context = std::ptr::from_ref(wakes).cast_mut().cast();
    assert_eq!(
        unsafe { sfm_client_set_waker(h.handle, Some(wake), context) },
        0
    );
    settle(wakes, 1);
    h.dispatch();
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_connect(h.handle, &mut op) }, 0);
    settle(wakes, 2);
    let req = h.request();
    assert_eq!(h.complete(&req, PAIR), 0);
    settle(wakes, 3);
    h.event();
    h.close();
    let after = wakes.load(Ordering::SeqCst);
    assert_eq!(
        unsafe { sfm_client_set_waker(h.handle, Some(wake), context) },
        STALE
    );
    assert_eq!(wakes.load(Ordering::SeqCst), after);
}
