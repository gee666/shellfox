use shellfox_mobile::*;
use std::{ffi::c_void, slice};

fn create() -> u64 {
    let mut handle = 0;
    assert_eq!(unsafe { sfm_terminal_create(80, 24, &mut handle) }, 0);
    assert_ne!(handle, 0);
    handle
}
fn submit(handle: u64, sequence: u64, bytes: &[u8]) -> i32 {
    unsafe { sfm_terminal_submit(handle, sequence, bytes.as_ptr(), bytes.len()) }
}
fn progress(handle: u64) -> SfmProgress {
    let mut result = SfmProgress::default();
    assert_eq!(unsafe { sfm_terminal_progress(handle, &mut result) }, 0);
    result
}
extern "C" fn bytes(context: *mut c_void, ptr: *const u8, length: usize) {
    // SAFETY: test owns the Vec for the entire synchronous callback.
    let result = unsafe { &mut *context.cast::<Vec<u8>>() };
    result.extend_from_slice(unsafe { slice::from_raw_parts(ptr, length) });
}
fn input(handle: u64, json: &str) -> (i32, Vec<u8>) {
    let mut result = Vec::<u8>::new();
    let status = unsafe {
        sfm_terminal_input(
            handle,
            json.as_ptr(),
            json.len(),
            Some(bytes),
            (&mut result as *mut Vec<u8>).cast(),
        )
    };
    (status, result)
}
extern "C" fn frame(context: *mut c_void, ptr: *const SfmFrame) {
    // SAFETY: C callback borrows a valid frame/cells/text until it returns.
    let result = unsafe { &mut *context.cast::<String>() };
    let frame = unsafe { &*ptr };
    assert_eq!(
        frame.cell_count,
        usize::from(frame.cols) * usize::from(frame.rows)
    );
    for cell in unsafe { slice::from_raw_parts(frame.cells, frame.cell_count) } {
        let text = unsafe { slice::from_raw_parts(cell.text, cell.text_length) };
        result.push_str(std::str::from_utf8(text).unwrap());
    }
}
#[test]
fn ordered_bounded_transport_is_credited_only_after_parse() {
    let handle = create();
    let chunk = vec![b'x'; MAX_CHUNK];
    for n in 1..=16 {
        assert_eq!(submit(handle, n, &chunk), 0);
    }
    assert_eq!(submit(handle, 17, b"x"), Status::Busy as i32);
    assert_eq!(progress(handle).accepted_sequence, 16);
    assert_eq!(progress(handle).consumed_sequence, 0);
    assert_eq!(sfm_terminal_pump(handle, None, std::ptr::null_mut()), 0);
    assert_eq!(progress(handle).consumed_sequence, 2);
    assert_eq!(submit(handle, 17, b"x"), 0);
    while progress(handle).queued_chunks > 0 {
        assert_eq!(sfm_terminal_pump(handle, None, std::ptr::null_mut()), 0);
    }
    assert_eq!(progress(handle).consumed_sequence, 17);
    assert_eq!(sfm_terminal_close(handle), 0);
    assert_eq!(submit(handle, 18, b"x"), Status::Stale as i32);
}
#[test]
fn engine_is_thread_confined_but_output_submission_is_not() {
    let handle = create();
    std::thread::spawn(move || {
        assert_eq!(submit(handle, 1, b"remote"), 0);
        assert_eq!(progress(handle).accepted_sequence, 1);
        assert_eq!(
            sfm_terminal_pump(handle, None, std::ptr::null_mut()),
            Status::WrongThread as i32
        );
        assert_eq!(sfm_terminal_close(handle), Status::WrongThread as i32);
    })
    .join()
    .unwrap();
    assert_eq!(sfm_terminal_pump(handle, None, std::ptr::null_mut()), 0);
    let mut text = String::new();
    assert_eq!(
        sfm_terminal_frame(handle, Some(frame), (&mut text as *mut String).cast()),
        0
    );
    assert!(text.starts_with("remote"));
    assert_eq!(sfm_terminal_close(handle), 0);
}
#[test]
fn owner_exit_drops_engine_and_invalidates_handle() {
    let handle = std::thread::spawn(create).join().unwrap();
    assert_eq!(submit(handle, 1, b"x"), Status::Stale as i32);
    let newer = create();
    assert_ne!(handle, newer);
    assert_eq!(sfm_terminal_close(newer), 0);
}
#[test]
fn ime_and_control_input_use_ghostty_and_not_literal_escapes() {
    let handle = create();
    assert_eq!(input(handle, r#"{"type":"editorReset"}"#), (0, vec![]));
    assert_eq!(
        input(handle, r#"{"type":"editor","committed":"日本"}"#),
        (0, "日本".as_bytes().to_vec())
    );
    assert_eq!(
        input(
            handle,
            r#"{"type":"key","event":{"physical":58,"action":"press","modifiers":0,"consumedModifiers":0,"text":"","unshiftedCodepoint":null,"composing":false}}"#
        ),
        (0, b"\r".to_vec())
    );
    assert_eq!(
        input(handle, r#"{"type":"editor","committed":"\u001b[A"}"#).0,
        Status::Invalid as i32
    );
    assert_eq!(sfm_terminal_close(handle), 0);
}
#[test]
fn selection_copy_links_and_scroll_stay_in_native_engine() {
    let handle = create();
    assert_eq!(
        submit(
            handle,
            1,
            b"hello \x1b]8;;https://example.test\x1b\\link\x1b]8;;\x1b\\"
        ),
        0
    );
    assert_eq!(sfm_terminal_pump(handle, None, std::ptr::null_mut()), 0);
    assert_eq!(
        sfm_terminal_select(
            handle,
            SfmSelection {
                kind: 1,
                start_x: 0,
                start_y: 0,
                end_x: 4,
                end_y: 0,
                rectangle: 0,
            }
        ),
        0
    );
    let mut copied = Vec::<u8>::new();
    assert_eq!(
        sfm_terminal_copy(handle, Some(bytes), (&mut copied as *mut Vec<u8>).cast()),
        0
    );
    assert_eq!(copied, b"hello");
    copied.clear();
    assert_eq!(
        sfm_terminal_hyperlink(
            handle,
            6,
            0,
            Some(bytes),
            (&mut copied as *mut Vec<u8>).cast()
        ),
        0
    );
    assert_eq!(copied, b"https://example.test");
    assert_eq!(sfm_terminal_scroll(handle, 10), 0);
    assert_eq!(sfm_terminal_close(handle), 0);
}
#[test]
fn plaintext_file_links_resolve_both_rows_in_the_owner_viewport() {
    let mut handle = 0;
    assert_eq!(unsafe { sfm_terminal_create(20, 4, &mut handle) }, 0);
    let path = "./src/deep/界/terminal.rs:12:4";
    let output = format!("{path}\r\nhttps://example.test\r\nnext\r\nlast\r\n");
    assert_eq!(submit(handle, 1, output.as_bytes()), 0);
    assert_eq!(sfm_terminal_pump(handle, None, std::ptr::null_mut()), 0);
    assert_eq!(sfm_terminal_scroll(handle, -10), 0);

    let at = |x, y| {
        let mut target = Vec::<u8>::new();
        assert_eq!(
            sfm_terminal_file_link(
                handle,
                x,
                y,
                Some(bytes),
                (&mut target as *mut Vec<u8>).cast()
            ),
            0
        );
        String::from_utf8(target).unwrap()
    };
    // The first row includes a two-cell glyph. Either cell and the continuation hit the same path.
    assert_eq!(at(11, 0), path);
    assert_eq!(at(12, 0), path);
    assert_eq!(at(1, 1), path);
    assert!(at(19, 1).is_empty());
    assert!(at(1, 2).is_empty());
    assert!(at(20, 0).is_empty());
    assert_eq!(sfm_terminal_close(handle), 0);
}

#[test]
fn owner_reset_drops_old_output_selection_and_local_credit() {
    let handle = create();
    assert_eq!(submit(handle, 1, b"old"), 0);
    assert_eq!(sfm_terminal_pump(handle, None, std::ptr::null_mut()), 0);
    assert_eq!(submit(handle, 2, b"must-not-appear"), 0);
    assert_eq!(
        sfm_terminal_select(
            handle,
            SfmSelection {
                kind: 4,
                start_x: 0,
                start_y: 0,
                end_x: 0,
                end_y: 0,
                rectangle: 0,
            }
        ),
        0
    );
    std::thread::spawn(move || {
        assert_eq!(
            sfm_terminal_reset(handle, 80, 24),
            Status::WrongThread as i32
        );
    })
    .join()
    .unwrap();
    assert_eq!(sfm_terminal_reset(handle, 0, 24), Status::Invalid as i32);
    assert_eq!(progress(handle).accepted_sequence, 2);
    assert_eq!(sfm_terminal_reset(handle, 80, 24), 0);
    let p = progress(handle);
    assert_eq!(
        (p.accepted_sequence, p.consumed_sequence, p.queued_chunks),
        (0, 0, 0)
    );
    let mut copied = Vec::<u8>::new();
    assert_eq!(
        sfm_terminal_copy(handle, Some(bytes), (&mut copied as *mut Vec<u8>).cast()),
        0
    );
    assert!(copied.is_empty());
    assert_eq!(submit(handle, 1, b"new"), 0);
    assert_eq!(sfm_terminal_pump(handle, None, std::ptr::null_mut()), 0);
    let mut text = String::new();
    assert_eq!(
        sfm_terminal_frame(handle, Some(frame), (&mut text as *mut String).cast()),
        0
    );
    assert!(text.starts_with("new"));
    assert!(!text.contains("old"));
    assert!(!text.contains("must-not-appear"));
    assert_eq!(sfm_terminal_close(handle), 0);
}

#[test]
fn c_layout_matches_header_on_64_bit_targets() {
    if std::mem::size_of::<usize>() == 8 {
        assert_eq!(std::mem::size_of::<SfmProgress>(), 24);
        assert_eq!(std::mem::size_of::<SfmCell>(), 32);
        assert_eq!(std::mem::size_of::<SfmFrame>(), 40);
        assert_eq!(std::mem::offset_of!(SfmCell, foreground), 16);
        assert_eq!(std::mem::offset_of!(SfmFrame, cells), 16);
    }
}

#[test]
fn rejects_null_and_oversized_c_inputs_without_dereferencing() {
    assert_eq!(
        unsafe { sfm_terminal_create(80, 24, std::ptr::null_mut()) },
        Status::Invalid as i32
    );
    let handle = create();
    assert_eq!(
        unsafe { sfm_terminal_submit(handle, 1, std::ptr::null(), MAX_CHUNK + 1) },
        Status::Invalid as i32
    );
    assert_eq!(
        unsafe {
            sfm_terminal_input(
                handle,
                std::ptr::null(),
                1,
                Some(bytes),
                std::ptr::null_mut(),
            )
        },
        Status::Invalid as i32
    );
    assert_eq!(
        sfm_terminal_frame(handle, None, std::ptr::null_mut()),
        Status::Invalid as i32
    );
    assert_eq!(
        sfm_terminal_resize(handle, 512, 512),
        Status::Invalid as i32
    );
    assert_eq!(submit(handle, 2, b"gap"), Status::Invalid as i32);
    assert_eq!(sfm_terminal_close(handle), 0);
}
