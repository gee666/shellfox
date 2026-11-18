use shellfox_mobile::*;
use std::{ffi::c_void, slice};

struct Probe {
    handle: u64,
    progress_status: i32,
    frame_status: i32,
    bytes: Vec<u8>,
}
extern "C" fn frame(_context: *mut c_void, _frame: *const SfmFrame) {}
extern "C" fn encoded(context: *mut c_void, ptr: *const u8, length: usize) {
    // SAFETY: this test owns Probe for the complete synchronous input callback.
    let probe = unsafe { &mut *context.cast::<Probe>() };
    let mut progress = SfmProgress::default();
    probe.progress_status = unsafe { sfm_terminal_progress(probe.handle, &mut progress) };
    probe.frame_status = sfm_terminal_frame(probe.handle, Some(frame), std::ptr::null_mut());
    probe
        .bytes
        .extend_from_slice(unsafe { slice::from_raw_parts(ptr, length) });
}

#[test]
fn input_forwarding_runs_after_engine_borrow_and_queue_lock_are_released() {
    let mut handle = 0;
    assert_eq!(unsafe { sfm_terminal_create(20, 5, &mut handle) }, 0);
    let mut probe = Probe {
        handle,
        progress_status: -1,
        frame_status: -1,
        bytes: Vec::new(),
    };
    let input = br#"{"type":"commit","text":"user input"}"#;
    assert_eq!(
        unsafe {
            sfm_terminal_input(
                handle,
                input.as_ptr(),
                input.len(),
                Some(encoded),
                (&mut probe as *mut Probe).cast(),
            )
        },
        0
    );
    assert_eq!(probe.bytes, b"user input");
    assert_eq!(probe.progress_status, 0);
    assert_eq!(probe.frame_status, 0);
    assert_eq!(sfm_terminal_close(handle), 0);
}
