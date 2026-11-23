use shellfox_mobile::*;
use std::{ffi::c_void, slice};

#[derive(Default)]
struct InputResult {
    frames: Vec<(Vec<u8>, u8)>,
    queries: Vec<(i32, i32)>,
    handle: u64,
}
extern "C" fn frame(_context: *mut c_void, _frame: *const SfmFrame) {}
extern "C" fn encoded(context: *mut c_void, ptr: *const u8, length: usize, user_initiated: u8) {
    // SAFETY: the test owns InputResult for the entire synchronous callback.
    let result = unsafe { &mut *context.cast::<InputResult>() };
    let bytes = unsafe { slice::from_raw_parts(ptr, length) }.to_vec();
    result.frames.push((bytes, user_initiated));
    let mut progress = SfmProgress::default();
    let progress_status = unsafe { sfm_terminal_progress(result.handle, &mut progress) };
    let frame_status = sfm_terminal_frame(result.handle, Some(frame), std::ptr::null_mut());
    result.queries.push((progress_status, frame_status));
}
struct Terminal {
    handle: u64,
    sequence: u64,
}
impl Terminal {
    fn new() -> Self {
        let mut handle = 0;
        assert_eq!(unsafe { sfm_terminal_create(80, 24, &mut handle) }, 0);
        Self {
            handle,
            sequence: 0,
        }
    }
    fn feed(&mut self, data: &[u8]) {
        self.sequence += 1;
        assert_eq!(
            unsafe { sfm_terminal_submit(self.handle, self.sequence, data.as_ptr(), data.len()) },
            0
        );
        assert_eq!(
            sfm_terminal_pump(self.handle, None, std::ptr::null_mut()),
            0
        );
    }
    fn input(&self, json: &str) -> (i32, InputResult) {
        let mut result = InputResult {
            handle: self.handle,
            ..Default::default()
        };
        let status = unsafe {
            sfm_terminal_input_ex(
                self.handle,
                json.as_ptr(),
                json.len(),
                Some(encoded),
                (&mut result as *mut InputResult).cast(),
            )
        };
        (status, result)
    }
    fn expect_hint(&self, json: &str, hint: u8) -> Vec<u8> {
        let (status, result) = self.input(json);
        assert_eq!(status, 0, "{json}");
        assert_eq!(result.frames.len(), 1, "expected one encoded frame: {json}");
        assert_eq!(result.frames[0].1, hint, "{json}");
        assert_eq!(
            result.queries,
            vec![(0, 0)],
            "callback must run after engine release"
        );
        result.frames.into_iter().next().unwrap().0
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        assert_eq!(sfm_terminal_close(self.handle), 0);
    }
}

#[test]
fn editor_paste_press_repeat_and_ctrl_space_are_length_delimited_user_input() {
    let terminal = Terminal::new();
    assert_eq!(
        terminal.expect_hint(r#"{"type":"editor","committed":"日本"}"#, 1),
        "日本".as_bytes()
    );
    assert_eq!(
        terminal.expect_hint(r#"{"type":"paste","text":"paste"}"#, 1),
        b"paste"
    );
    for action in ["press", "repeat"] {
        let json = format!(r#"{{"type":"key","event":{{"physical":78,"action":"{action}","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false}}}}"#);
        assert_eq!(terminal.expect_hint(&json, 1), b"\x1b[A");
    }
    // Control-Space is a real one-byte NUL, not an empty C string or JSON text.
    assert_eq!(terminal.expect_hint(r#"{"type":"key","event":{"physical":63,"action":"press","modifiers":2,"consumedModifiers":0,"text":" ","unshiftedCodepoint":32,"composing":false}}"#, 1), [0]);
}

#[test]
fn kitty_key_release_is_forwarded_without_claiming_meaningful_user_input() {
    let mut terminal = Terminal::new();
    terminal.feed(b"\x1b[>11u");
    let bytes = terminal.expect_hint(r#"{"type":"key","event":{"physical":20,"action":"release","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":97,"composing":false}}"#, 0);
    assert!(!bytes.is_empty());
}

#[test]
fn mouse_hover_is_passive_while_drag_click_release_and_wheel_are_active() {
    let mut terminal = Terminal::new();
    terminal.feed(b"\x1b[?1003h\x1b[?1006h");
    let mouse = |action: &str, button: Option<&str>, x: u32, pressed: bool| {
        serde_json::json!({"type":"mouse","action":action,"button":button,"x":x,"y":1,
            "cellWidth":8,"cellHeight":16,"anyButtonPressed":pressed})
        .to_string()
    };
    assert_eq!(
        terminal.expect_hint(&mouse("motion", None, 1, false), 0),
        b"\x1b[<35;1;1M"
    );
    terminal.expect_hint(&mouse("press", Some("left"), 1, true), 1);
    terminal.expect_hint(&mouse("motion", Some("left"), 17, true), 1);
    terminal.expect_hint(&mouse("release", Some("left"), 17, false), 1);
    terminal.expect_hint(&mouse("press", Some("wheelDown"), 17, false), 1);
}

#[test]
fn editor_reset_and_empty_encoder_output_never_call_the_forwarder() {
    let mut terminal = Terminal::new();
    for json in [
        r#"{"type":"editorReset"}"#,
        r#"{"type":"editor","committed":""}"#,
        r#"{"type":"paste","text":""}"#,
        r#"{"type":"key","event":{"physical":58,"action":"release","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false}}"#,
    ] {
        let (status, result) = terminal.input(json);
        assert_eq!(status, 0);
        assert!(result.frames.is_empty(), "{json}");
    }
    terminal.feed(b"\x1b[?2004h");
    let (status, result) = terminal.input(r#"{"type":"paste","text":""}"#);
    assert_eq!(status, 0);
    assert!(result.frames.iter().all(|(_, hint)| *hint == 0));
}

#[test]
fn input_metadata_does_not_bypass_engine_thread_ownership() {
    let terminal = Terminal::new();
    let handle = terminal.handle;
    let (status, calls) = std::thread::spawn(move || {
        let json = br#"{"type":"editor","committed":"x"}"#;
        let mut result = InputResult {
            handle,
            ..Default::default()
        };
        let status = unsafe {
            sfm_terminal_input_ex(
                handle,
                json.as_ptr(),
                json.len(),
                Some(encoded),
                (&mut result as *mut InputResult).cast(),
            )
        };
        (status, result.frames.len())
    })
    .join()
    .unwrap();
    assert_eq!(status, Status::WrongThread as i32);
    assert_eq!(calls, 0);
}

#[test]
fn rejects_forged_intent_and_invalid_events_without_calling_forwarder() {
    let terminal = Terminal::new();
    for json in [
        r#"{"type":"editor","committed":"hello","userInitiated":true}"#,
        r#"{"type":"key","event":{"physical":58,"action":"release","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false},"user_initiated":1}"#,
        r#"{"type":"editor","committed":"\u001b[0n"}"#,
        r#"{"type":"mouse","action":"motion","x":0,"y":0,"cellWidth":0,"cellHeight":16,"anyButtonPressed":false}"#,
    ] {
        let (status, result) = terminal.input(json);
        assert_eq!(status, Status::Invalid as i32, "{json}");
        assert!(result.frames.is_empty());
    }
    assert_eq!(
        unsafe {
            sfm_terminal_input_ex(
                terminal.handle,
                std::ptr::null(),
                1,
                Some(encoded),
                std::ptr::null_mut(),
            )
        },
        Status::Invalid as i32
    );
    let json = br#"{"type":"editor","committed":"x"}"#;
    assert_eq!(
        unsafe {
            sfm_terminal_input_ex(
                terminal.handle,
                json.as_ptr(),
                json.len(),
                None,
                std::ptr::null_mut(),
            )
        },
        Status::Invalid as i32
    );
}
