use super::*;
const SETTLED: &str = "2026-01-01T00:00:00.000Z";
#[derive(Default)]
struct Forwarded {
    bytes: Vec<u8>,
    hint: u8,
    operation: u64,
    status: i32,
    progress: i32,
    frame: i32,
}
struct Forward {
    client: u64,
    terminal: u64,
    calls: Vec<Forwarded>,
}
extern "C" fn frame(_: *mut c_void, _: *const crate::SfmFrame) {}
extern "C" fn forward(context: *mut c_void, bytes: *const u8, length: usize, hint: u8) {
    // The encoder borrows neither engine nor queue when this callback runs.
    let context = unsafe { &mut *context.cast::<Forward>() };
    let mut call = Forwarded {
        bytes: unsafe { slice::from_raw_parts(bytes, length) }.to_vec(),
        hint,
        ..Default::default()
    };
    let mut progress = crate::SfmProgress::default();
    call.progress = unsafe { crate::sfm_terminal_progress(context.terminal, &mut progress) };
    call.frame = crate::sfm_terminal_frame(context.terminal, Some(frame), ptr::null_mut());
    call.status = unsafe {
        sfm_client_terminal_write_ex(
            context.client,
            context.terminal,
            bytes,
            length,
            hint,
            &mut call.operation,
        )
    };
    context.calls.push(call);
}
fn host_state(h: &mut Harness, view: &NativeView, token: &str, modes: Option<&str>) {
    let mut operation = 0;
    assert_eq!(unsafe { sfm_client_poll(h.handle, &mut operation) }, 0);
    let request = h.request();
    let mut events = vec![
        json!({"type":"terminal","event":{"type":"settlement","tabId":TAB,
        "generation":"30000000-0000-4000-8000-000000000001","settledAt":token}}),
    ];
    if let Some(modes) = modes {
        events.push(json!({"type":"terminal","event":{"type":"data","tabId":TAB,
        "generation":"30000000-0000-4000-8000-000000000001","controlEpoch":1,"sequence":2,"data":modes}}));
    }
    let response = serde_json::to_vec(&json!({"ok":true,"value":{"events":events}})).unwrap();
    assert_eq!(h.complete(&request, &response), 0);
    h.event();
    crate::engine::with_engine(view.0, crate::engine::pump).unwrap();
}
fn write_ex(h: &Harness, view: &NativeView, bytes: &[u8], hint: u8) -> (i32, u64) {
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
fn written(h: &mut Harness, request: &NativeRequest) {
    assert_eq!(
        h.complete(request, br#"{"ok":true,"value":{"written":true}}"#),
        0
    );
    let event = h.event();
    assert_eq!(event["status"], 0);
    assert_eq!(event["internal"], true);
    assert_eq!(event["result"], json!({"ok":true,"value":{"written":true}}));
}
#[test]
fn validated_encoder_hint_forwards_unchanged_after_engine_borrow_into_native_fifo() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    host_state(
        &mut h,
        &view,
        SETTLED,
        Some("\x1b[>31u\x1b[?1003h\x1b[?1006h\x1b[?2004h"),
    );
    let mut context = Forward {
        client: h.handle,
        terminal: view.0,
        calls: Vec::new(),
    };
    let mouse = |action, button, pressed| {
        json!({"type":"mouse","action":action,"button":button,
        "x":16,"y":32,"cellWidth":8,"cellHeight":16,"anyButtonPressed":pressed})
    };
    let cases = [
        (json!({"type":"commit","text":"🙂"}), 1),
        (json!({"type":"paste","text":"paste"}), 1),
        (
            json!({"type":"key","key":{"kind":"character","value":"a"},"text":"a","action":"press"}),
            1,
        ),
        (
            json!({"type":"key","key":{"kind":"character","value":"a"},"text":"a","action":"repeat"}),
            1,
        ),
        (
            json!({"type":"key","key":{"kind":"character","value":"a"},"text":"a","action":"release"}),
            0,
        ),
        (mouse("motion", Value::Null, false), 0),
        (mouse("press", json!("left"), true), 1),
        (mouse("motion", json!("left"), true), 1),
        (mouse("release", json!("left"), false), 1),
        (mouse("press", json!("wheelUp"), false), 1),
    ];
    for (event, hint) in cases {
        let input = serde_json::to_vec(&event).unwrap();
        let before = context.calls.len();
        assert_eq!(
            unsafe {
                crate::sfm_terminal_input_ex(
                    view.0,
                    input.as_ptr(),
                    input.len(),
                    Some(forward),
                    ptr::from_mut(&mut context).cast(),
                )
            },
            0
        );
        if context.calls.len() == before && event["type"] == "key" && event["action"] == "release" {
            // Ghostty may suppress a release in the active keyboard protocol.
            // Empty output must not create a client operation at all.
            h.dispatch();
            assert!(h.mock.requests.is_empty());
            continue;
        }
        assert_eq!(
            context.calls.len(),
            before + 1,
            "encoder returned no bytes for {event}"
        );
        let call = &context.calls[before];
        assert!(!call.bytes.is_empty());
        assert_eq!(call.hint, hint);
        assert_eq!((call.progress, call.frame, call.status), (0, 0, 0));
        let request = h.request();
        assert_eq!(
            request.body["payload"]["data"].as_str().unwrap().as_bytes(),
            call.bytes
        );
        assert_eq!(request.body["payload"]["userInitiated"], hint == 1);
        assert_eq!(request.body["payload"]["settledAt"], SETTLED);
        written(&mut h, &request);
    }
    for event in [
        json!({"type":"preedit","text":"marked"}),
        json!({"type":"commit","text":""}),
        json!({"type":"paste","text":""}),
    ] {
        let input = serde_json::to_vec(&event).unwrap();
        let before = context.calls.len();
        assert_eq!(
            unsafe {
                crate::sfm_terminal_input_ex(
                    view.0,
                    input.as_ptr(),
                    input.len(),
                    Some(forward),
                    ptr::from_mut(&mut context).cast(),
                )
            },
            0
        );
        if event["type"] == "preedit" {
            assert_eq!(context.calls.len(), before);
        } else if context.calls.len() != before {
            // An empty bracketed paste can still encode wrapper bytes, but its
            // user-input hint must remain false. Never fabricate an empty send.
            assert_eq!(context.calls.len(), before + 1);
            let call = &context.calls[before];
            assert!(!call.bytes.is_empty());
            assert_eq!(call.hint, 0);
            assert_eq!(call.status, 0);
            let request = h.request();
            assert_eq!(request.body["payload"]["userInitiated"], false);
            written(&mut h, &request);
        }
    }
}
#[test]
fn invalid_hint_accepts_no_id_and_does_not_read_an_invalid_span() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    for hint in [2, 255] {
        assert_eq!(write_ex(&h, &view, b"x", hint), (INVALID, 0));
    }
    let mut operation = 99;
    assert_eq!(
        unsafe {
            sfm_client_terminal_write_ex(
                h.handle,
                view.0,
                ptr::null(),
                usize::MAX,
                2,
                &mut operation,
            )
        },
        INVALID
    );
    assert_eq!(operation, 0);
    h.dispatch();
    assert!(h.mock.requests.is_empty());
}
#[test]
fn ex_false_is_explicit_and_legacy_write_delegates_true_without_changing_frozen_token() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    host_state(&mut h, &view, SETTLED, None);
    assert_eq!(write_ex(&h, &view, b"first", 1).0, 0);
    let first = h.request();
    assert_eq!(write_ex(&h, &view, b"\x1b[97;1:3u", 0).0, 0);
    let newer = "2026-01-01T00:00:01.000Z";
    host_state(&mut h, &view, newer, None);
    let mut operation = 0;
    assert_eq!(
        unsafe {
            sfm_client_terminal_write(h.handle, view.0, b"legacy".as_ptr(), 6, &mut operation)
        },
        0
    );
    written(&mut h, &first);
    let release = h.request();
    assert_eq!(release.body["payload"]["userInitiated"], false);
    assert_eq!(release.body["payload"]["settledAt"], SETTLED);
    written(&mut h, &release);
    let legacy = h.request();
    assert_eq!(legacy.body["payload"]["userInitiated"], true);
    assert_eq!(legacy.body["payload"]["settledAt"], newer);
    written(&mut h, &legacy);
}
#[test]
fn ctrl_space_nul_is_length_delimited_from_real_encoder_through_typed_request() {
    let mut h = Harness::new();
    h.connect();
    let view = NativeView::attach(&mut h);
    host_state(&mut h, &view, SETTLED, None);
    let mut context = Forward {
        client: h.handle,
        terminal: view.0,
        calls: Vec::new(),
    };
    let input = br#"{"type":"key","key":{"kind":"character","value":" "},"action":"press","modifiers":2,"text":" "}"#;
    assert_eq!(
        unsafe {
            crate::sfm_terminal_input_ex(
                view.0,
                input.as_ptr(),
                input.len(),
                Some(forward),
                ptr::from_mut(&mut context).cast(),
            )
        },
        0
    );
    assert_eq!(context.calls.len(), 1);
    let call = &context.calls[0];
    assert_eq!(call.bytes, [0]);
    assert_eq!(call.hint, 1);
    assert_eq!(call.status, 0);
    let request = h.request();
    let wire = registry_lookup(h.handle)
        .unwrap()
        .state
        .lock()
        .unwrap()
        .requests
        .get(&request.id)
        .unwrap()
        .body
        .clone();
    assert!(wire
        .windows(b"\"data\":\"\\u0000\"".len())
        .any(|bytes| bytes == b"\"data\":\"\\u0000\""));
    assert!(
        !wire.contains(&0),
        "NUL is escaped JSON data, not a C-string terminator"
    );
    assert_eq!(
        request.body["payload"]["data"].as_str().unwrap().as_bytes(),
        [0]
    );
    assert_eq!(request.body["payload"]["userInitiated"], true);
    assert_eq!(request.body["payload"]["settledAt"], SETTLED);
    written(&mut h, &request);
}
