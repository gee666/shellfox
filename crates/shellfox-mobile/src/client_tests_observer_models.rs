use super::*;
#[test]
fn canonical_43_methods_keep_v2_facts_and_profile_denials_without_v1_coercion() {
    let mut h = new_observer();
    let handshake = connect_trace(&mut h);
    let mut native_acks = Vec::new();
    let view = view(&h);
    let methods = golden_json("methods");
    let methods = methods.as_array().unwrap();
    assert_eq!(methods.len(), 43);
    let mut denied = 0;
    let mut captured = Vec::new();
    let mut mismatches = Vec::new();
    for case in methods {
        let method = case["method"].as_str().unwrap();
        let command = json!({"method":method,"payload":case["request"]["payload"]});
        if case["allowed"] == false || method == "acknowledgeTerminal" {
            assert_eq!(operation(&h, command), (INVALID, 0), "{method}");
            denied += 1;
            captured.push(json!({"method":method,"accepted":false,"status":INVALID}));
            continue;
        }
        assert_eq!(operation(&h, command).0, 0, "{method}");
        let request = h.request();
        assert_eq!(request.path, "/v2/request");
        assert_eq!(request.body, case["request"], "{method}");
        assert_eq!(json_reply(&h, &request, &case["response"]), 0);
        let event = h.event();
        assert_eq!(event["status"], 0, "{method}");
        assert_eq!(event["profile"], "observer-v2");
        assert_eq!(event["internal"], false);
        let mut expected = case["response"].clone();
        expected.as_object_mut().unwrap().remove("binding");
        if method == "attachTerminal" {
            expected["value"]["recovery"]
                .as_object_mut()
                .unwrap()
                .remove("data");
        }
        captured.push(json!({"method":method,"accepted":true,"request":request.body,"event":event,"expected":expected}));
        if !same_json(&event["result"], &expected) {
            mismatches.push(method.to_owned());
        }
        if method == "getSnapshot" {
            assert_eq!(event["result"]["value"], golden_json("snapshot")["value"]);
            assert!(event["result"]["value"].get("sessions").is_none());
        }
        if method == "attachTerminal" {
            reset(&h, &view);
            pump(&view);
            native_acks.push(ack_recovery_trace(&mut h, &view));
        }
        if method == "detachTerminal" {
            reset(&h, &view);
        }
    }
    capture_for_ts(&captured, &handshake, &native_acks);
    assert!(
        mismatches.is_empty(),
        "CABI must preserve keys/nulls/facts, allowing numeric spelling only: {mismatches:?}"
    );
    assert_eq!(denied, 8);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    let mut operation_id = 99;
    assert_eq!(
        unsafe {
            sfm_client_terminal_write_ex(h.handle, view.0, b"x".as_ptr(), 1, 1, &mut operation_id)
        },
        INVALID
    );
    assert_eq!(operation_id, 0);
    assert_eq!(
        unsafe { sfm_client_terminal_resize(h.handle, view.0, 90, 30, &mut operation_id) },
        INVALID
    );
}
#[test]
fn v1_attach_shapes_and_forged_bound_replies_fail_closed() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    for payload in [
        json!({"tabId":OBS_TAB}),
        json!({"tabId":OBS_TAB,"afterSequence":0}),
        json!({"tabId":OBS_TAB,"resume":null,"controlEpoch":1}),
    ] {
        assert_eq!(
            operation(&h, json!({"method":"attachTerminal","payload":payload})),
            (INVALID, 0)
        );
    }
    assert_eq!(
        operation(&h, json!({"method":"getSnapshot","payload":{}})).0,
        0
    );
    let request = h.request();
    let mut response = golden_json("snapshot");
    response["binding"]["connectionId"] = json!("40000000-0000-4000-8000-000000000002");
    assert_eq!(json_reply(&h, &request, &response), 0);
    let event = h.event();
    assert_eq!(event["status"], PROTOCOL);
    assert!(event["result"].is_null());
    assert!(event["connection"]["binding"].is_null());
    assert_eq!(
        crate::engine::lookup(view.0)
            .unwrap()
            .queue
            .lock()
            .unwrap()
            .accepted,
        0
    );
}
#[test]
fn server_delivery_epoch_and_input_claim_cannot_smuggle_authority() {
    for (field, invalid) in [
        ("deliveryEpoch", json!(0)),
        ("deliveryEpoch", json!(9007199254740992_u64)),
        ("generation", json!("60000000000040008000000000000001")),
    ] {
        let mut h = new_observer();
        connect(&mut h);
        let view = view(&h);
        assert_eq!(
            operation(
                &h,
                json!({"method":"attachTerminal","payload":{"tabId":OBS_TAB,"resume":null}})
            )
            .0,
            0
        );
        let request = h.request();
        let mut response = golden_json("attach");
        response["value"]["delivery"][field] = invalid;
        assert_eq!(json_reply(&h, &request, &response), 0);
        assert_eq!(h.event()["status"], PROTOCOL);
        assert_eq!(
            crate::engine::lookup(view.0)
                .unwrap()
                .queue
                .lock()
                .unwrap()
                .accepted,
            0
        );
    }
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    assert_eq!(
        operation(
            &h,
            json!({"method":"attachTerminal","payload":{"tabId":OBS_TAB,"resume":null}})
        )
        .0,
        0
    );
    let request = h.request();
    let mut response = golden_json("attach");
    response["value"]["input"] = json!({"control":"owned","controlEpoch":1});
    assert_eq!(json_reply(&h, &request, &response), 0);
    assert_eq!(h.event()["status"], PROTOCOL);
    assert_eq!(
        crate::engine::lookup(view.0)
            .unwrap()
            .queue
            .lock()
            .unwrap()
            .accepted,
        0
    );
}
#[test]
fn native_engine_role_is_exclusive_across_profiles_and_late_close_callbacks_are_stale() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    let legacy = Harness::new();
    assert_eq!(
        unsafe { sfm_client_bind_terminal(legacy.handle, TAB.as_ptr(), TAB.len(), view.0) },
        BUSY
    );
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), view.0) },
        INVALID
    );
    assert_eq!(
        operation(&h, json!({"method":"getSnapshot","payload":{}})).0,
        0
    );
    let request = h.request();
    let handle = h.handle;
    h.close();
    assert!(h.mock.cancelled.contains(&(request.generation, request.id)));
    assert_eq!(
        unsafe {
            sfm_observer_v2_complete_http(
                handle,
                request.generation,
                request.id,
                0,
                404,
                ptr::null(),
                0,
            )
        },
        STALE
    );
}
