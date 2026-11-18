use shellfox_mobile::*;
use std::{ffi::c_void, ptr};

extern "C" fn input(context: *mut c_void, _: *const u8, _: usize) {
    // SAFETY: test retains this bool for the synchronous ABI call.
    unsafe {
        *context.cast::<bool>() = true;
    }
}
extern "C" fn input_ex(context: *mut c_void, _: *const u8, _: usize, _: u8) {
    unsafe {
        *context.cast::<bool>() = true;
    }
}
unsafe extern "C" fn cancel(_: *mut c_void, _: u64, _: u64, _: u64) {}

#[test]
fn observer_binding_denies_common_engine_input_and_viewport_resize_without_network() {
    let peer = b"fixture-peer";
    let identity = b"10000000-0000-4000-8000-000000000001";
    let tab = b"20000000-0000-4000-8000-000000000001";
    let (mut client, mut terminal) = (0, 0);
    assert_eq!(
        unsafe {
            sfm_observer_v2_create(
                peer.as_ptr(),
                peer.len(),
                identity.as_ptr(),
                identity.len(),
                &mut client,
            )
        },
        0
    );
    assert_eq!(unsafe { sfm_terminal_create(80, 24, &mut terminal) }, 0);
    assert_eq!(
        unsafe { sfm_observer_v2_bind_terminal(client, tab.as_ptr(), tab.len(), terminal) },
        0
    );
    let mut called = false;
    let context = ptr::from_mut(&mut called).cast();
    let json = br#"{"type":"commit","text":"x"}"#;
    assert_eq!(
        unsafe { sfm_terminal_input(terminal, json.as_ptr(), json.len(), Some(input), context) },
        Status::Invalid as i32
    );
    assert_eq!(
        unsafe {
            sfm_terminal_input_ex(terminal, json.as_ptr(), json.len(), Some(input_ex), context)
        },
        Status::Invalid as i32
    );
    assert!(!called);
    assert_eq!(
        sfm_terminal_resize(terminal, 100, 40),
        Status::Invalid as i32
    );
    // Reset is owner-only recovery plumbing, not a canonical layout grant.
    assert_eq!(sfm_terminal_reset(terminal, 100, 40), 0);
    assert_eq!(sfm_observer_v2_unbind_terminal(client, terminal), 0);
    assert_eq!(sfm_terminal_resize(terminal, 80, 24), 0);
    assert_eq!(sfm_terminal_close(terminal), 0);
    assert_eq!(
        unsafe { sfm_client_close(client, Some(cancel), ptr::null_mut()) },
        0
    );
}
