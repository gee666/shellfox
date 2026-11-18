use super::*;
fn ready(h: &Harness, view: &NativeView) -> bool {
    let mut value = 99;
    assert_eq!(
        unsafe { sfm_observer_v2_terminal_recovery_ready(h.handle, view.0, &mut value) },
        0
    );
    value == 1
}
#[test]
fn canonical_recovery_requires_owner_reset_and_consumption_before_distinct_ack() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    let event = attach(&mut h, &golden_json("attach"));
    assert_eq!(event["result"]["value"]["kind"], "attached");
    assert!(event["result"]["value"]["input"].is_null());
    assert_eq!(
        event["result"]["value"]["layout"]["kind"],
        "fixedCanonicalGrid"
    );
    assert!(event["result"]["value"]["recovery"].get("data").is_none());
    assert!(event["result"]["value"].get("control").is_none());
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), STALE);
    assert_eq!(poll(&h), (BUSY, 0));
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_observer_v2_terminal_reset_ticket(h.handle, view.0, &mut ticket) },
        0
    );
    crate::engine::reset(view.0, 20, 5).unwrap();
    assert_eq!(
        sfm_observer_v2_terminal_reset_done(h.handle, view.0, ticket),
        INVALID
    );
    reset(&h, &view);
    assert!(!ready(&h, &view));
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), EMPTY);
    pump(&view);
    assert!(ready(&h, &view));
    let handle = h.handle;
    let engine = view.0;
    assert_eq!(
        thread::spawn(move || sfm_observer_v2_terminal_consumed(handle, engine))
            .join()
            .unwrap(),
        4
    );
    let reference = golden_json("attach")["value"]["delivery"].clone();
    assert_eq!(
        operation(
            &h,
            json!({"method":"acknowledgeTerminal","payload":{"delivery":reference,
        "ack":{"kind":"recovery","recoveryId":"80000000-0000-4000-8000-000000000001","sequence":4}}})
        ),
        (INVALID, 0)
    );
    ack_recovery(&mut h, &view);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), EMPTY);
    assert_eq!(poll(&h).0, 0);
    let request = h.request();
    assert_eq!(
        http(
            &h,
            &request,
            200,
            include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/events.json")
        ),
        0
    );
    let event = h.event();
    let coarse = event["result"]["value"]["events"].as_array().unwrap();
    assert_eq!(coarse.len(), 1);
    assert_eq!(coarse[0]["event"]["type"], "sourceEnded");
    assert!(coarse[0]["event"].get("exitCode").is_none());
    assert!(!event.to_string().contains("QQBC"));
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), EMPTY);
    pump(&view);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), 0);
    let ack = h.request();
    assert_eq!(
        ack.body["payload"]["ack"],
        json!({"kind":"data","sequence":5})
    );
    assert_eq!(
        http(
            &h,
            &ack,
            200,
            include_bytes!("../../shellfox-protocol/tests/fixtures/observer_v2/acknowledged.json")
        ),
        0
    );
    assert_eq!(h.event()["status"], 0);
}
#[test]
fn four_mib_recovery_is_segmented_and_never_credited_at_a_partial_native_ticket() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    let mut bytes = b"\x18\x1bc".to_vec();
    bytes.resize(4 * 1024 * 1024, b'x');
    let encoded: String = bytes
        .chunks(shellfox_protocol::FILE_CHUNK_BYTES)
        .map(|part| shellfox_protocol::encode_chunk(part).unwrap())
        .collect();
    let mut attachment = golden_json("attach");
    attachment["value"]["recovery"]["data"] = json!(encoded);
    let event = attach(&mut h, &attachment);
    assert!(event.to_string().len() < 4096);
    reset(&h, &view);
    let shared = crate::engine::lookup(view.0).unwrap();
    assert_eq!(shared.queue.lock().unwrap().accepted, 16);
    pump(&view);
    assert!(!ready(&h, &view));
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), EMPTY);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    for _ in 0..7 {
        pump(&view);
    }
    assert!(ready(&h, &view));
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), 0);
    let ack = h.request();
    assert_eq!(ack.body["payload"]["ack"]["kind"], "recovery");
    assert_eq!(ack.body["payload"]["ack"]["sequence"], 4); // NOT native ticket 16.
}
#[test]
fn layout_change_requests_recovery_from_consumed_cursor_without_resizing_host() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    attach(&mut h, &golden_json("attach"));
    reset(&h, &view);
    pump(&view);
    ack_recovery(&mut h, &view);
    assert_eq!(poll(&h).0, 0);
    let request = h.request();
    let mut events = golden_json("events");
    events["value"]["events"].as_array_mut().unwrap().pop();
    events["value"]["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"terminal","event":golden_json("layout")}));
    assert_eq!(json_reply(&h, &request, &events), 0);
    let event = h.event();
    assert_eq!(event["result"]["value"]["events"][0]["type"], "reattach");
    assert_eq!(
        event["result"]["value"]["events"][0]["request"]["resume"]["afterSequence"],
        4
    );
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), STALE);
    assert!(event["resetGeometry"].as_object().unwrap().is_empty());
    reset(&h, &view);
    assert!(!ready(&h, &view));
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn reconnect_clears_recovery_cache_and_rejects_old_callback_or_reset_ticket() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    attach(&mut h, &golden_json("attach"));
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_observer_v2_terminal_reset_ticket(h.handle, view.0, &mut ticket) },
        0
    );
    let new_id = b"20000000-0000-4000-8000-000000000002";
    assert_eq!(
        unsafe { sfm_observer_v2_reconnect(h.handle, new_id.as_ptr(), new_id.len()) },
        0
    );
    assert_eq!(
        sfm_observer_v2_terminal_reset_done(h.handle, view.0, ticket),
        STALE
    );
    let mut current = 0;
    assert_eq!(
        unsafe { sfm_observer_v2_terminal_reset_ticket(h.handle, view.0, &mut current) },
        0
    );
    assert_ne!(current, ticket);
    let (mut cols, mut rows) = (99, 99);
    assert_eq!(
        unsafe {
            sfm_observer_v2_terminal_reset_grid(h.handle, view.0, current, &mut cols, &mut rows)
        },
        EMPTY
    );
    assert_eq!((cols, rows), (0, 0));
    reset(&h, &view);
    assert!(!ready(&h, &view));
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), STALE);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn corrupted_local_parser_grid_cannot_grant_observer_credit() {
    let mut h = new_observer();
    connect(&mut h);
    let view = view(&h);
    attach(&mut h, &golden_json("attach"));
    reset(&h, &view);
    // Fault injection below the public owner API, not a supported observer fit.
    crate::engine::with_engine(view.0, |owned| {
        owned
            .terminal
            .resize(20, 5)
            .map_err(|_| crate::Status::Engine)
    })
    .unwrap();
    pump(&view);
    assert_eq!(sfm_observer_v2_terminal_consumed(h.handle, view.0), INVALID);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
