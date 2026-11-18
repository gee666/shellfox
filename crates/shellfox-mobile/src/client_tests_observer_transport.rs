use super::*;
#[test]
fn explicit_creator_keeps_v1_defaults_and_completion_routes_isolated() {
    let mut legacy = Harness::new();
    let mut observer = new_observer();
    assert_eq!(sfm_client_abi_version(), 1);
    assert_eq!(sfm_observer_v2_abi_version(), 1);
    for (h, expected) in [(&legacy, (1, 7)), (&observer, (2, 1))] {
        let (mut profile, mut caps) = (0, 0);
        assert_eq!(
            unsafe { sfm_client_profile(h.handle, &mut profile, &mut caps) },
            0
        );
        assert_eq!((profile, caps), expected);
    }
    let mut operation_id = 99;
    assert_eq!(
        unsafe { sfm_client_connect(observer.handle, &mut operation_id) },
        INVALID
    );
    assert_eq!(operation_id, 0);
    assert_eq!(
        legacy
            .operation(json!({"method":"getSnapshot","payload":{}}))
            .0,
        0
    );
    assert_eq!(legacy.event()["status"], STALE);
    assert_eq!(
        operation(&legacy, json!({"method":"getSnapshot","payload":{}})),
        (INVALID, 0)
    );
    assert_eq!(discover(&observer, 0).0, 0);
    let request = observer.request();
    let discovery =
        include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/discovery.json");
    assert_eq!(observer.complete(&request, discovery), INVALID);
    assert_eq!(http(&observer, &request, 200, discovery), 0);
    observer.event();
    assert_eq!(
        unsafe { sfm_client_connect(legacy.handle, &mut operation_id) },
        0
    );
    let request = legacy.request();
    assert_eq!(http(&legacy, &request, 200, PAIR), INVALID);
    assert_eq!(legacy.complete(&request, PAIR), 0);
    let event = legacy.event();
    assert_eq!(event["connection"]["status"], "connected");
    assert!(event.get("profile").is_none());
}
#[test]
fn actual_pending_discover_404_only_offers_visible_validated_legacy() {
    let mut h = new_observer();
    assert_eq!(discover(&h, 1).0, 0);
    let request = h.request();
    assert_eq!(request.path, "/v2/discover");
    assert_eq!(http(&h, &request, 404, &[]), 0);
    let legacy = h.request();
    assert_eq!(legacy.path, "/v1/discover");
    assert_eq!(legacy.body, json!({}));
    let response =
        include_bytes!("../../shellfox-client/tests/fixtures/remote-discover-success.json");
    assert_eq!(http(&h, &legacy, 200, response), 0);
    let event = h.event();
    assert_eq!(event["profile"], "observer-v2");
    assert_eq!(event["result"]["kind"], "legacyAvailable");
    assert_eq!(event["connection"]["status"], "legacyAvailable");
    assert!(event["connection"]["binding"].is_null());
    h.dispatch();
    assert!(
        h.mock.requests.is_empty(),
        "no implicit v1 pair/attach or control"
    );
    assert_eq!(
        h.operation(json!({"method":"getSnapshot","payload":{}})),
        (INVALID, 0)
    );
}
#[test]
fn status_validation_never_parses_english_or_json_as_http_status() {
    for status in [401, 403, 500] {
        let mut h = new_observer();
        assert_eq!(discover(&h, 1).0, 0);
        let request = h.request();
        assert_eq!(http(&h, &request, status, &[]), 0);
        assert_eq!(h.event()["status"], NETWORK);
        h.dispatch();
        assert!(h.mock.requests.is_empty());
    }
    let mut h = new_observer();
    assert_eq!(discover(&h, 1).0, 0);
    let request = h.request();
    for status in [0, 99, 600] {
        assert_eq!(http(&h, &request, status, &[]), INVALID);
    }
    assert_eq!(http(&h, &request, 404, b"HTTP 404 Not Found"), INVALID);
    assert_eq!(
        unsafe {
            sfm_observer_v2_complete_http(
                h.handle,
                request.generation,
                request.id,
                3,
                404,
                ptr::null(),
                0,
            )
        },
        INVALID
    );
    assert_eq!(
        http(
            &h,
            &request,
            200,
            br#"{"httpStatus":404,"error":"Not Found"}"#
        ),
        0
    );
    assert_eq!(h.event()["status"], PROTOCOL);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn cancelled_or_expired_discovery_cannot_offer_legacy_from_a_late_404() {
    let mut h = new_observer();
    let (status, op) = discover(&h, 1);
    assert_eq!(status, 0);
    let request = h.request();
    assert_eq!(sfm_client_cancel(h.handle, op), 0);
    assert_eq!(http(&h, &request, 404, &[]), STALE);
    assert_eq!(h.event()["status"], CANCELLED);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    assert_eq!(discover(&h, 1).0, 0);
    let request = h.request();
    registry_lookup(h.handle)
        .unwrap()
        .state
        .lock()
        .unwrap()
        .requests
        .get_mut(&request.id)
        .unwrap()
        .deadline = Instant::now() - Duration::from_secs(1);
    assert_eq!(http(&h, &request, 404, &[]), 0);
    assert_eq!(h.event()["status"], NETWORK);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn request_404_and_post_pair_reconnect_never_downgrade_or_retry() {
    let mut h = new_observer();
    connect(&mut h);
    assert_eq!(
        operation(&h, json!({"method":"getSnapshot","payload":{}})).0,
        0
    );
    let request = h.request();
    assert_eq!(request.path, "/v2/request");
    assert_eq!(http(&h, &request, 404, &[]), 0);
    assert_eq!(h.event()["status"], NETWORK);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    let new_id = b"20000000-0000-4000-8000-000000000002";
    assert_eq!(
        unsafe { sfm_observer_v2_reconnect(h.handle, new_id.as_ptr(), new_id.len()) },
        0
    );
    assert_eq!(discover(&h, 1).0, 0);
    let request = h.request();
    assert_eq!(http(&h, &request, 404, &[]), 0);
    assert_eq!(h.event()["status"], NETWORK);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn explicit_disconnect_withdraws_the_connection_without_closing_host_sources() {
    let mut h = new_observer(); connect(&mut h); let view = view(&h);
    attach(&mut h, &golden_json("attach"));
    let mut op = 0; assert_eq!(unsafe { sfm_observer_v2_disconnect(h.handle, &mut op) }, 0);
    let request = h.request(); assert_eq!(request.path, "/v2/disconnect");
    let expected: Value = serde_json::from_slice(include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/disconnect-connection.json")).unwrap();
    assert_eq!(request.body, expected);
    let reply = include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/disconnected.json");
    assert_eq!(http(&h, &request, 200, reply), 0); let event = h.event();
    assert_eq!(event["operationId"], op.to_string()); assert_eq!(event["status"], 0);
    assert_eq!(event["result"], serde_json::from_slice::<Value>(reply).unwrap());
    assert_eq!(event["connection"]["status"], "disconnected"); assert!(event["connection"]["binding"].is_null());
    assert!(event["resetGeometry"].as_object().unwrap().is_empty());
    reset(&h, &view); assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), STALE);
    h.dispatch(); assert!(h.mock.requests.is_empty());
}
#[test]
fn explicit_attempt_withdrawal_fences_uncertain_pair_and_preserves_actual_attempt() {
    let mut h = new_observer(); assert_eq!(discover(&h, 0).0, 0); let request = h.request();
    assert_eq!(http(&h, &request, 200, include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/discovery.json")), 0); h.event();
    let mut pair_op = 0; assert_eq!(unsafe { sfm_observer_v2_pair(h.handle, ATTEMPT.as_ptr(), ATTEMPT.len(), &mut pair_op) }, 0);
    let pair = h.request(); let mut op = 0; assert_eq!(unsafe { sfm_observer_v2_disconnect(h.handle, &mut op) }, 0);
    let request = h.request(); assert_eq!(request.path, "/v2/disconnect");
    let expected: Value = serde_json::from_slice(include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/disconnect-attempt.json")).unwrap();
    assert_eq!(request.body, expected);
    assert_eq!(http(&h, &request, 200, include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/disconnected.json")), 0);
    let mut withdrew = false;
    for _ in 0..2 { let event = h.event(); if event["operationId"].as_str() == Some(op.to_string().as_str()) { assert_eq!(event["status"], 0); withdrew = true; } }
    assert!(withdrew);
    assert_eq!(http(&h, &pair, 200, include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/pair-selection.json")), STALE);
    h.dispatch(); assert!(h.mock.requests.is_empty());
}
#[test]
fn invalid_profile_options_and_nil_or_shorthand_ids_fail_before_transport() {
    let mut h = new_observer();
    assert_eq!(discover(&h, 2), (INVALID, 0));
    let mut id = 99;
    let nil = b"00000000-0000-0000-0000-000000000000";
    assert_eq!(
        unsafe { sfm_observer_v2_pair(h.handle, nil.as_ptr(), nil.len(), &mut id) },
        INVALID
    );
    assert_eq!(id, 0);
    let short = b"30000000000040008000000000000001";
    assert_eq!(
        unsafe { sfm_observer_v2_pair(h.handle, short.as_ptr(), short.len(), &mut id) },
        INVALID
    );
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
