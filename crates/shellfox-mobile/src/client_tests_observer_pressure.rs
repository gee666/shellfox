use super::*;
#[test]
fn slow_consumer_stops_polling_and_retries_failed_ack_without_more_pump() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    attach(&mut h, &golden_json("attach"));
    reset(&h, &view);
    pump(&view);
    ack_recovery(&mut h, &view);
    let encoded = shellfox_protocol::encode_chunk(&vec![b'x'; 16384]).unwrap();
    let reference = golden_json("attach")["value"]["delivery"].clone();
    for batch in 0..4 {
        assert_eq!(poll(&h).0, 0);
        let request = h.request();
        let events: Vec<_> = (0..16)
            .map(|i| {
                json!({"type":"terminal","event":{"type":"data","delivery":reference,
            "sequence":5 + batch * 16 + i,"encoding":"base64","data":encoded}})
            })
            .collect();
        let response = json!({"version":2,"binding":golden_json("binding"),"ok":true,"value":{"events":events}});
        assert_eq!(json_reply(&h, &request, &response), 0);
        let event = h.event();
        assert_eq!(event["result"]["value"]["events"], json!([]));
    }
    assert_eq!(poll(&h), (BUSY, 0));
    assert_eq!(
        crate::engine::lookup(view.0)
            .unwrap()
            .queue
            .lock()
            .unwrap()
            .chunks
            .len(),
        64
    );
    pump(&view);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), 0);
    let ack = h.request();
    assert_eq!(
        ack.body["payload"]["ack"],
        json!({"kind":"data","sequence":36})
    );
    let failure = json!({"version":2,"binding":golden_json("binding"),"ok":false,"error":{
        "code":"INTERNAL","message":"retry ACK","retryable":true,"detail":{"kind":"application"}}});
    assert_eq!(json_reply(&h, &ack, &failure), 0);
    assert_eq!(h.event()["result"]["ok"], false);
    h.dispatch();
    assert!(h.mock.requests.is_empty(), "no automatic retry");
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), 0);
    let retry = h.request();
    assert_eq!(retry.body, ack.body);
    assert_eq!(json_reply(&h, &retry, &golden_json("ack")), 0);
    assert_eq!(h.event()["status"], 0);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), EMPTY);
}
#[test]
fn revocation_fences_inflight_ack_and_never_infers_process_exit() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    attach(&mut h, &golden_json("attach"));
    reset(&h, &view);
    pump(&view);
    ack_recovery(&mut h, &view);
    assert_eq!(poll(&h).0, 0);
    let request = h.request();
    assert_eq!(json_reply(&h, &request, &golden_json("events")), 0);
    h.event();
    pump(&view);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), 0);
    let ack = h.request();
    assert_eq!(poll(&h).0, 0);
    let request = h.request();
    let response = json!({"version":2,"binding":golden_json("binding"),"ok":true,"value":{"events":[{"type":"terminal",
        "event":{"type":"revoked","delivery":golden_json("attach")["value"]["delivery"],"reason":"approvalRevoked"}}]}});
    assert_eq!(json_reply(&h, &request, &response), 0);
    let mut revoked = false;
    for _ in 0..2 {
        let event = h.event();
        revoked |= event["result"]["value"]["events"][0]["event"]["type"] == "revoked";
    }
    assert!(revoked);
    assert_eq!(json_reply(&h, &ack, &golden_json("ack")), STALE);
    reset(&h, &view);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), STALE);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn oversized_carriers_and_unsupported_native_geometry_fail_before_credit() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    let huge = vec![b' '; shellfox_protocol::MAX_REQUEST_BYTES + 1];
    let mut operation_id = 99;
    assert_eq!(
        unsafe {
            sfm_observer_v2_operation(h.handle, huge.as_ptr(), huge.len(), &mut operation_id)
        },
        INVALID
    );
    assert_eq!(operation_id, 0);
    assert_eq!(
        operation(&h, json!({"method":"getSnapshot","payload":{}})).0,
        0
    );
    let request = h.request();
    let huge = vec![b' '; shellfox_protocol::MAX_RESPONSE_BYTES + 1];
    assert_eq!(http(&h, &request, 200, &huge), INVALID);
    assert_eq!(json_reply(&h, &request, &golden_json("snapshot")), 0);
    h.event();
    assert_eq!(
        operation(
            &h,
            json!({"method":"attachTerminal","payload":{"tabId":OBS_TAB,"resume":null}})
        )
        .0,
        0
    );
    let request = h.request();
    let mut attachment = golden_json("attach");
    attachment["value"]["recovery"]["cols"] = json!(500);
    attachment["value"]["recovery"]["rows"] = json!(500);
    assert_eq!(json_reply(&h, &request, &attachment), 0);
    let event = h.event();
    assert_eq!(event["status"], INVALID);
    assert!(event["result"].is_null());
    assert!(event["resetGeometry"].as_object().unwrap().is_empty());
    reset(&h, &view);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), STALE);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn structured_business_unsupported_is_not_a_downgrade_signal() {
    let mut h = new_observer();
    assert_eq!(discover(&h, 1).0, 0);
    let request = h.request();
    let response = json!({"version":2,"ok":false,"error":{"code":"UNSUPPORTED","message":"HTTP 404 Not Found",
        "retryable":false,"detail":{"kind":"application"}}});
    assert_eq!(json_reply(&h, &request, &response), 0);
    let event = h.event();
    assert_eq!(event["result"], response);
    assert_ne!(event["connection"]["status"], "legacyAvailable");
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
