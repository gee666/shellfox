//! History from a 0.9.15 Electron host: its xterm-serialized snapshot and the
//! full redraws a main-screen TUI such as pi sends after a resize.
use super::*;
const GENERATION: &str = "30000000-0000-4000-8000-000000000001";

/// The shape of 0.9.15's ScreenMirror.snapshot: every history and screen row
/// joined by CRLF, then the cursor, split into 4096-unit chunks ending at `last`.
fn electron_snapshot(history: usize, cols: u16, rows: u16, last: u64) -> Value {
    let lines: Vec<String> = (0..history + usize::from(rows))
        .map(|i| format!("\x1b[3{}mrow {i:05}\x1b[0m", i % 7 + 1))
        .collect();
    let data = format!("{}\x1b[?25h\x1b[{rows};1H", lines.join("\r\n"));
    let parts: Vec<&str> = data
        .as_bytes()
        .chunks(4096)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    let first = last + 1 - parts.len() as u64;
    let chunks: Vec<Value> = parts
        .iter()
        .enumerate()
        .map(|(i, data)| json!({"type":"data","tabId":TAB,"generation":GENERATION,"sequence":first + i as u64,"data":data}))
        .collect();
    json!({"ok":true,"value":{"control":"owned","controlEpoch":1,"tabId":TAB,
        "sessionId":"40000000-0000-4000-8000-000000000001","generation":GENERATION,
        "firstSequence":first,"lastSequence":last,"chunks":chunks,"truncated":true,"snapshot":true,
        "state":"open","exitCode":null,"cols":cols,"rows":rows,"lifetime":"app-owned"}})
}
fn data(sequence: u64, data: &str) -> Value {
    json!({"type":"terminal","event":{"type":"data","tabId":TAB,"generation":GENERATION,
        "sequence":sequence,"data":data,"controlEpoch":1}})
}
/// pi-tui's full redraw: clear, home, clear history, then every content line.
fn full_redraw(lines: usize) -> String {
    let body: Vec<String> = (0..lines).map(|i| format!("line {i:05}")).collect();
    format!(
        "\x1b[?2026h\x1b[2J\x1b[H\x1b[3J{}\x1b[?2026l",
        body.join("\r\n")
    )
}
fn history(engine: u64) -> u64 {
    crate::engine::with_engine(engine, |owned| {
        owned
            .terminal
            .recovery_info()
            .map_err(|_| crate::Status::Engine)
    })
    .unwrap()
    .scrollback_rows
}
/// Attaches with `request` and installs the host's answer like the Android owner.
fn attach(h: &mut Harness, engine: u64, request: Value, answer: &Value) {
    let (status, _) = h.operation(json!({"method":"attachTerminal","payload":request}));
    assert_eq!(status, 0);
    let req = h.request();
    assert_eq!(req.body["method"], "attachTerminal");
    assert!(req.body["payload"].get("afterSequence").is_none());
    assert_eq!(h.complete(&req, &serde_json::to_vec(answer).unwrap()), 0);
    let event = h.event();
    assert_eq!(event["status"], 0);
    reset_for_event(h, engine, &event);
    settle(h, engine);
}
/// Parses everything queued and acknowledges it, as the owner's ticks do.
fn settle(h: &mut Harness, engine: u64) {
    loop {
        crate::engine::with_engine(engine, crate::engine::pump).unwrap();
        match sfm_client_terminal_consumed(h.handle, engine) {
            0 => {
                let ack = h.request();
                assert_eq!(ack.body["method"], "acknowledgeTerminal");
                let ok = br#"{"ok":true,"value":{"acknowledged":true}}"#;
                assert_eq!(h.complete(&ack, ok), 0);
                h.event();
            }
            EMPTY => break,
            status => panic!("unexpected credit status {status}"),
        }
    }
    let mut ready = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_recovery_ready(h.handle, engine, &mut ready) },
        0
    );
    assert_eq!(ready, 1);
}
fn poll(h: &mut Harness, events: Vec<Value>) -> Value {
    let mut op = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut op) }, 0);
    let req = h.request();
    assert_eq!(req.path, "/v1/events");
    let body = json!({"ok":true,"value":{"events":events}});
    assert_eq!(h.complete(&req, &serde_json::to_vec(&body).unwrap()), 0);
    h.event()["result"]["value"]["events"].clone()
}
struct Engine(u64);
impl Drop for Engine {
    fn drop(&mut self) {
        crate::engine::close(self.0).unwrap();
    }
}
fn bound(h: &Harness) -> Engine {
    let engine = Engine(crate::engine::create(45, 30).unwrap());
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), engine.0) },
        0
    );
    engine
}

#[test]
fn electron_snapshot_history_becomes_engine_scrollback() {
    let mut h = Harness::new();
    h.connect();
    let engine = bound(&h);
    attach(
        &mut h,
        engine.0,
        json!({"tabId":TAB}),
        &electron_snapshot(1500, 120, 40, 900),
    );
    assert_eq!(history(engine.0), 1500);
}

#[test]
fn resize_redraws_keep_history_and_an_overflowed_window_reattaches() {
    let mut h = Harness::new();
    h.connect();
    let engine = bound(&h);
    attach(
        &mut h,
        engine.0,
        json!({"tabId":TAB}),
        &electron_snapshot(200, 120, 40, 50),
    );
    // The phone's grid: the owner resizes its engine, then the host's PTY.
    assert_eq!(crate::sfm_terminal_resize(engine.0, 45, 30), 0);
    let mut op = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_resize(h.handle, engine.0, 45, 30, &mut op) },
        0
    );
    let resize = h.request();
    assert_eq!(
        h.complete(&resize, br#"{"ok":true,"value":{"resized":true}}"#),
        0
    );
    h.event();
    // The TUI repaints its whole conversation at the new width.
    assert_eq!(poll(&mut h, vec![data(51, &full_redraw(600))]), json!([]));
    settle(&mut h, engine.0);
    assert_eq!(history(engine.0), 600 - 30);
    // A longer redraw outgrows 0.9.15's view window: the host delivers the
    // start, then stops this view until it reattaches.
    let overflow = json!({"type":"terminal","event":{"type":"error","tabId":TAB,
        "generation":GENERATION,"controlEpoch":1,"error":{"code":"NATIVE_UNAVAILABLE",
        "message":"Terminal output exceeded the view replay window. Reattach to the current bounded output; older output was discarded.",
        "retryable":true}}});
    let events = poll(&mut h, vec![data(52, &full_redraw(100)), overflow]);
    assert_eq!(events.as_array().unwrap().len(), 1, "{events}");
    assert_eq!(events[0]["type"], "reattach");
    let request = events[0]["request"].clone();
    assert_eq!(request["generation"], GENERATION);
    // The fresh snapshot carries the mirror's history at the phone's width.
    attach(
        &mut h,
        engine.0,
        request,
        &electron_snapshot(2400, 45, 30, 400),
    );
    assert_eq!(history(engine.0), 2400);
}
