use super::*;
fn ready(h: &Harness, engine: u64) -> bool {
    let mut ready = 99;
    assert_eq!(
        unsafe { sfm_client_terminal_recovery_ready(h.handle, engine, &mut ready) },
        0
    );
    ready == 1
}
fn screen(engine: u64) -> shellfox_ghostty_vt::Screen {
    crate::engine::with_engine(engine, |owned| {
        owned.terminal.render().map_err(|_| crate::Status::Engine)
    })
    .unwrap()
}
#[test]
fn initial_recovery_requires_exact_host_grid_before_feed_and_defers_viewport_fit() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView(crate::engine::create(80, 24).unwrap());
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), view.0) },
        0
    );
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        0
    );
    let request = h.request();
    let mut attachment: Value = serde_json::from_slice(include_bytes!(
        "../../shellfox-client/tests/fixtures/terminal-attach-owned-success.json"
    ))
    .unwrap();
    let chunks = [
        format!("\x1b[2J\x1b[H{}🙂", "x".repeat(199)),
        "\x1b[".into(),
        "2;199Hé\x1b[c".into(),
    ];
    attachment["value"]["cols"] = json!(200);
    attachment["value"]["rows"] = json!(80);
    attachment["value"]["lastSequence"] = json!(3);
    attachment["value"]["chunks"] = json!(chunks
        .iter()
        .enumerate()
        .map(|(i, data)| json!({"type":"data","tabId":TAB,
        "generation":"30000000-0000-4000-8000-000000000001","sequence":i+1,"data":data}))
        .collect::<Vec<_>>());
    let response = serde_json::to_vec(&attachment).unwrap();
    assert_eq!(h.complete(&request, &response), 0);
    let event = h.event();
    let key = view.0.to_string();
    assert_eq!(event["resetGeometry"][&key], json!({"cols":200,"rows":80}));
    assert_eq!(
        crate::engine::lookup(view.0)
            .unwrap()
            .queue
            .lock()
            .unwrap()
            .accepted,
        0
    );
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_reset_ticket(h.handle, view.0, &mut ticket) },
        0
    );
    let (mut cols, mut rows) = (0, 0);
    assert_eq!(
        unsafe { sfm_client_terminal_reset_grid(h.handle, view.0, ticket, &mut cols, &mut rows) },
        0
    );
    assert_eq!((cols, rows), (200, 80));
    // The local viewport is 80x24. It is explicitly not the recovery parser grid.
    crate::engine::reset(view.0, 80, 24).unwrap();
    assert_eq!(
        sfm_client_terminal_reset_done(h.handle, view.0, ticket),
        INVALID
    );
    assert!(!ready(&h, view.0));
    crate::engine::reset(view.0, 200, 80).unwrap();
    assert_eq!(sfm_client_terminal_reset_done(h.handle, view.0, ticket), 0);
    assert!(!ready(&h, view.0));
    let mut operation = 99;
    assert_eq!(
        unsafe { sfm_client_terminal_resize(h.handle, view.0, 80, 24, &mut operation) },
        BUSY
    );
    assert_eq!(operation, 0);
    crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
    assert!(ready(&h, view.0));
    let mut reference = shellfox_ghostty_vt::Terminal::new(200, 80).unwrap();
    reference.feed(chunks.concat().as_bytes()).unwrap();
    reference.drain_replies().unwrap();
    let expected = reference.render().unwrap();
    let actual = screen(view.0);
    assert_eq!((actual.cols, actual.rows), (200, 80));
    assert_eq!(actual.cells, expected.cells);
    assert_eq!(actual.cursor, expected.cursor);
    assert_eq!(sfm_client_terminal_consumed(h.handle, view.0), 0);
    let ack = h.request();
    assert_eq!(ack.body["payload"]["sequence"], 3);
    assert_eq!(
        h.complete(&ack, br#"{"ok":true,"value":{"acknowledged":true}}"#),
        0
    );
    h.event();
    // Protocol strings are valid UTF-8. Exercise raw native chunk boundaries too:
    // CSI and a four-byte Unicode character continue across separate owner pumps.
    let pieces: [&[u8]; 3] = [b"\x1b[3;", b"3H\xf0\x9f", b"\x8c\x9f"];
    for (i, bytes) in pieces.iter().enumerate() {
        assert_eq!(
            unsafe {
                crate::sfm_terminal_submit(view.0, 4 + i as u64, bytes.as_ptr(), bytes.len())
            },
            0
        );
        crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
        reference.feed(bytes).unwrap();
        reference.drain_replies().unwrap();
    }
    let actual = screen(view.0);
    let expected = reference.render().unwrap();
    assert_eq!(actual.cells, expected.cells);
    assert_eq!(actual.cursor, expected.cursor);
    h.dispatch();
    assert!(
        h.mock.requests.is_empty(),
        "mirror queries never become remote input"
    );
}
#[test]
fn blocked_view_may_fit_locally_but_cannot_resize_remote_host() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView(crate::engine::create(80, 24).unwrap());
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), view.0) },
        0
    );
    assert_eq!(
        h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}))
            .0,
        0
    );
    let request = h.request();
    let blocked =
        include_bytes!("../../shellfox-client/tests/fixtures/terminal-attach-blocked-success.json");
    assert_eq!(h.complete(&request, blocked), 0);
    h.event();
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_reset_ticket(h.handle, view.0, &mut ticket) },
        0
    );
    crate::engine::reset(view.0, 80, 24).unwrap();
    assert_eq!(sfm_client_terminal_reset_done(h.handle, view.0, ticket), 0);
    assert!(ready(&h, view.0));
    let mut operation = 99;
    assert_eq!(
        unsafe { sfm_client_terminal_resize(h.handle, view.0, 120, 40, &mut operation) },
        STALE
    );
    assert_eq!(operation, 0);
    assert_eq!(crate::sfm_terminal_resize(view.0, 120, 40), 0);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
/// Attaches a pi-style screen at a 60x50 host grid: history, a footer on the
/// last rows and the cursor parked in the editor above it. Then fits the
/// recovered engine through `fits` and returns the rows holding the footer.
fn recover_parked_footer(fits: &[(u16, u16)]) -> Vec<usize> {
    let mut h = Harness::new();
    h.connect();
    // Both native owners create the engine at this placeholder before layout.
    let view = NativeView(crate::engine::create(80, 24).unwrap());
    assert_eq!(
        unsafe { sfm_client_bind_terminal(h.handle, TAB.as_ptr(), TAB.len(), view.0) },
        0
    );
    h.operation(json!({"method":"attachTerminal","payload":{"tabId":TAB}}));
    let request = h.request();
    let mut attachment: Value = serde_json::from_slice(include_bytes!(
        "../../shellfox-client/tests/fixtures/terminal-attach-owned-success.json"
    ))
    .unwrap();
    let history: String = (0..60).map(|i| format!("line {i}\r\n")).collect();
    let data = format!("{history}editor\r\n\r\nFOOTER-1\r\nFOOTER-2\x1b[3A\r");
    attachment["value"]["cols"] = json!(60);
    attachment["value"]["rows"] = json!(50);
    attachment["value"]["lastSequence"] = json!(1);
    attachment["value"]["chunks"] = json!([{"type":"data","tabId":TAB,
        "generation":"30000000-0000-4000-8000-000000000001","sequence":1,"data":data}]);
    let response = serde_json::to_vec(&attachment).unwrap();
    assert_eq!(h.complete(&request, &response), 0);
    h.event();
    let mut ticket = 0;
    assert_eq!(
        unsafe { sfm_client_terminal_reset_ticket(h.handle, view.0, &mut ticket) },
        0
    );
    crate::engine::reset(view.0, 60, 50).unwrap();
    assert_eq!(sfm_client_terminal_reset_done(h.handle, view.0, ticket), 0);
    crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
    assert!(ready(&h, view.0));
    for &(cols, rows) in fits {
        assert_eq!(crate::sfm_terminal_resize(view.0, cols, rows), 0);
    }
    let fitted = screen(view.0);
    let cols = usize::from(fitted.cols);
    (0..usize::from(fitted.rows))
        .filter(|row| {
            fitted.cells[row * cols..(row + 1) * cols]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .starts_with("FOOTER")
        })
        .collect()
}
#[test]
fn recovered_screen_keeps_its_footer_only_when_fitted_straight_to_the_view() {
    // The reopen bug: the placeholder fit pushes the rows above the parked
    // cursor into scrollback, and growing back to the view adds blank rows
    // below instead. The host grid never changed, so no redraw repairs it.
    assert_eq!(recover_parked_footer(&[(80, 24), (60, 50)]), vec![22, 23]);
    // The native owners keep the host grid until the view has measured itself.
    assert_eq!(recover_parked_footer(&[]), vec![48, 49]);
    assert_eq!(recover_parked_footer(&[(60, 50)]), vec![48, 49]);
    assert_eq!(recover_parked_footer(&[(60, 40)]), vec![38, 39]);
}
