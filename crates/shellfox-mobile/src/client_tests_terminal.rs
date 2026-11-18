use super::*;
const ATTACH: &[u8] =
    include_bytes!("../../shellfox-client/tests/fixtures/terminal-attach-owned-success.json");
struct Engine(u64);
impl Engine {
    fn new() -> Self {
        Self(crate::engine::create(80, 24).unwrap())
    }
    fn pump(&self) {
        crate::engine::with_engine(self.0, crate::engine::pump).unwrap();
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        crate::engine::close(self.0).unwrap();
    }
}
fn bind(h: &Harness, e: &Engine) {
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), e.0) },
        0
    );
}
fn attach(h: &mut Harness) -> Value {
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        0
    );
    let req = h.request();
    assert_eq!(req.body["method"], "attachTerminal");
    assert_eq!(h.complete(&req, ATTACH), 0);
    let event = h.event();
    let terminal: u64 = event["resetTerminals"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let shared = crate::engine::lookup(terminal).unwrap();
    let accepted = shared.queue.lock().unwrap().accepted;
    if accepted == 0 {
        reset_for_event(h, terminal, &event);
    }
    event
}
#[test]
fn native_engine_credit_is_after_parser_consumption_and_never_js_output() {
    let mut h = Harness::new();
    h.connect();
    let e = Engine::new();
    bind(&h, &e);
    let result = attach(&mut h);
    assert_eq!(result["status"], 0);
    assert!(result["result"]["value"].get("chunks").is_none());
    assert!(!result.to_string().contains("hello"));
    assert_eq!(sfm_client_terminal_consumed(h.handle, e.0), EMPTY);
    let shared = crate::engine::lookup(e.0).unwrap();
    assert_eq!(shared.queue.lock().unwrap().accepted, 1);
    assert_eq!(shared.queue.lock().unwrap().consumed, 0);
    e.pump();
    assert_eq!(sfm_client_terminal_consumed(h.handle, e.0), 0);
    let ack = h.request();
    assert_eq!(ack.body["method"], "acknowledgeTerminal");
    assert_eq!(ack.body["payload"]["sequence"], 1);
    assert_eq!(sfm_client_terminal_consumed(h.handle, e.0), BUSY);
    assert_eq!(
        h.complete(&ack, br#"{"ok":true,"value":{"acknowledged":true}}"#),
        0
    );
    let ack_event = h.event();
    assert_eq!(ack_event["status"], 0);
    assert_eq!(ack_event["internal"], true);
    assert_eq!(sfm_client_terminal_consumed(h.handle, e.0), EMPTY);
    let live = include_bytes!("../../shellfox-client/tests/fixtures/terminal-events-success.json");
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut op) }, 0);
    let poll = h.request();
    assert_eq!(poll.path, "/v1/events");
    assert_eq!(h.complete(&poll, live), 0);
    let event = h.event();
    assert_eq!(event["result"]["value"]["events"], json!([]));
    assert_eq!(event["internal"], true);
    assert_eq!(sfm_client_terminal_consumed(h.handle, e.0), EMPTY);
    e.pump();
    assert_eq!(sfm_client_terminal_consumed(h.handle, e.0), 0);
    let ack = h.request();
    assert_eq!(ack.body["payload"]["sequence"], 2);
}
#[test]
fn native_write_resize_use_rust_lease_and_control_revocation_stops_input() {
    let mut h = Harness::new();
    h.connect();
    let e = Engine::new();
    bind(&h, &e);
    attach(&mut h);
    e.pump();
    let mut op = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_write(h.handle, e.0, b"x".as_ptr(), 1, &mut op) },
        0
    );
    let write = h.request();
    assert_eq!(write.body["payload"]["controlEpoch"], 1);
    assert_eq!(write.body["method"], "writeTerminal");
    assert_eq!(
        h.complete(&write, br#"{"ok":true,"value":{"written":true}}"#),
        0
    );
    assert_eq!(h.event()["internal"], true);
    // Even an exact current lease from a successful native write cannot grant
    // the JS command channel raw terminal-input or credit capability.
    assert_eq!(
        h.operation(json!({"method":"writeTerminal","payload":write.body["payload"]})),
        (INVALID, 0)
    );
    assert_eq!(
        h.operation(json!({"method":"acknowledgeTerminal","payload":{
        "tabId":write.body["payload"]["tabId"], "generation":write.body["payload"]["generation"],
        "controlEpoch":write.body["payload"]["controlEpoch"], "sequence":1}})),
        (INVALID, 0)
    );
    h.dispatch();
    assert!(h.mock.requests.is_empty());
    assert_eq!(
        unsafe { sfm_client_terminal_resize(h.handle, e.0, 90, 30, &mut op) },
        0
    );
    let resize = h.request();
    assert_eq!(resize.body["payload"]["cols"], 90);
    assert_eq!(
        h.complete(&resize, br#"{"ok":true,"value":{"resized":true}}"#),
        0
    );
    assert_eq!(h.event()["internal"], true);
    assert_eq!(
        h.operation(json!({"method":"resizeTerminal","payload":resize.body["payload"]}))
            .0,
        0
    );
    let ui_resize = h.request();
    assert_eq!(
        h.complete(&ui_resize, br#"{"ok":true,"value":{"resized":true}}"#),
        0
    );
    assert_eq!(h.event()["internal"], false);
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut op) }, 0);
    let poll = h.request();
    let blocked =
        include_bytes!("../../shellfox-client/tests/fixtures/terminal-control-event-success.json");
    assert_eq!(h.complete(&poll, blocked), 0);
    h.event();
    assert_eq!(
        unsafe { sfm_client_terminal_write(h.handle, e.0, b"x".as_ptr(), 1, &mut op) },
        STALE
    );
    assert_eq!(
        unsafe { sfm_client_terminal_resize(h.handle, e.0, 90, 30, &mut op) },
        STALE
    );
}
#[test]
fn reconnect_requires_owner_reset_ticket_and_old_events_cannot_reset_new_recovery() {
    let mut h = Harness::new();
    h.connect();
    let e = Engine::new();
    bind(&h, &e);
    attach(&mut h);
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_reconnect(h.handle, &mut op) }, 0);
    let req = h.request();
    assert_eq!(h.complete(&req, PAIR), 0);
    let reset = h.event();
    assert_eq!(reset["resetTerminals"], json!([e.0.to_string()]));
    assert!(reset["resetGeometry"].get(e.0.to_string()).is_none());
    assert_eq!(reset["recoveryReady"], json!([]));
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        BUSY
    );
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_reset_ticket(h.handle, e.0, &mut ticket) },
        0
    );
    let (mut cols, mut rows) = (99, 99);
    assert_eq!(
        unsafe { sfm_client_terminal_reset_grid(h.handle, e.0, ticket, &mut cols, &mut rows) },
        EMPTY
    );
    assert_eq!((cols, rows), (0, 0));
    assert_eq!(sfm_client_terminal_reset_done(h.handle, e.0, ticket), BUSY);
    crate::engine::reset(e.0, 80, 24).unwrap();
    assert_eq!(sfm_client_terminal_reset_done(h.handle, e.0, ticket), 0);
    let mut ready = 99;
    assert_eq!(
        unsafe { sfm_client_terminal_recovery_ready(h.handle, e.0, &mut ready) },
        0
    );
    assert_eq!(ready, 0); // A fence-only reset is not an installed recovery.
    attach(&mut h);
    // A delayed duplicate reset event must be ignored, not clear the new parser.
    let mut current = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_reset_ticket(h.handle, e.0, &mut current) },
        EMPTY
    );
    assert_eq!(sfm_client_terminal_reset_done(h.handle, e.0, ticket), STALE);
    assert_eq!(
        crate::engine::lookup(e.0)
            .unwrap()
            .queue
            .lock()
            .unwrap()
            .accepted,
        1
    );
}
#[test]
fn repeated_attachment_is_held_until_matching_owner_reset() {
    let mut h = Harness::new();
    h.connect();
    let e = Engine::new();
    bind(&h, &e);
    attach(&mut h);
    let held = attach(&mut h);
    assert_eq!(held["resetTerminals"], json!([e.0.to_string()]));
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_reset_ticket(h.handle, e.0, &mut ticket) },
        0
    );
    let client = h.handle;
    let terminal = e.0;
    assert_eq!(
        thread::spawn(move || sfm_client_terminal_reset_done(client, terminal, ticket))
            .join()
            .unwrap(),
        4
    );
    crate::engine::reset(e.0, 80, 24).unwrap();
    assert_eq!(sfm_client_terminal_reset_done(h.handle, e.0, ticket), 0);
    assert_eq!(
        crate::engine::lookup(e.0)
            .unwrap()
            .queue
            .lock()
            .unwrap()
            .accepted,
        1
    );
}
#[test]
fn caller_cannot_forge_ack_credit() {
    let h = Harness::new();
    assert_eq!(h.operation(json!({"method":"acknowledgeTerminal","payload":{
        "tabId":TAB,"generation":"30000000-0000-4000-8000-000000000001","controlEpoch":1,"sequence":5}})).0, INVALID);
}
