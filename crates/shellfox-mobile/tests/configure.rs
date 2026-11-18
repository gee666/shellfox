use shellfox_mobile::*;
use std::ffi::c_void;

extern "C" fn colors(context: *mut c_void, ptr: *const SfmFrame) {
    // SAFETY: test owns the result until the synchronous callback returns.
    let result = unsafe { &mut *context.cast::<[u32; 2]>() };
    let frame = unsafe { &*ptr };
    assert!(frame.cell_count > 0);
    let cell = unsafe { &*frame.cells };
    *result = [cell.foreground, cell.background];
}
fn configure(handle: u64, value: &str) -> i32 {
    unsafe { sfm_terminal_configure(handle, value.as_ptr(), value.len()) }
}
fn render_colors(handle: u64) -> [u32; 2] {
    let mut result = [0u32; 2];
    assert_eq!(
        sfm_terminal_frame(handle, Some(colors), (&mut result as *mut [u32; 2]).cast()),
        0
    );
    result
}
#[test]
fn strict_native_configuration_updates_real_parser_and_survives_reset() {
    let mut handle = 0;
    assert_eq!(unsafe { sfm_terminal_create(20, 5, &mut handle) }, 0);
    let config = r##"{"foreground":"#123456","background":"#654321","cursor":"#abcdef","selectionBackground":"#223344"}"##;
    assert_eq!(configure(handle, config), 0);
    assert_eq!(render_colors(handle), [0x123456, 0x654321]);
    assert_eq!(
        configure(
            handle,
            r##"{"foreground":"#123456","background":"#654321","palette":["#000000"]}"##
        ),
        Status::Invalid as i32
    );
    assert_eq!(
        configure(
            handle,
            r##"{"foreground":"#ffffff","background":"#000000","hostSecret":"bad"}"##
        ),
        Status::Invalid as i32
    );
    assert_eq!(render_colors(handle), [0x123456, 0x654321]);
    assert_eq!(sfm_terminal_reset(handle, 20, 5), 0);
    assert_eq!(render_colors(handle), [0x123456, 0x654321]);
    assert_eq!(sfm_terminal_close(handle), 0);
}

#[test]
fn rejects_color_configuration_on_non_owner_thread() {
    let mut handle = 0;
    assert_eq!(unsafe { sfm_terminal_create(20, 5, &mut handle) }, 0);
    std::thread::spawn(move || {
        assert_eq!(
            configure(
                handle,
                r##"{"foreground":"#123456","background":"#654321"}"##
            ),
            Status::WrongThread as i32
        );
    })
    .join()
    .unwrap();
    assert_eq!(sfm_terminal_close(handle), 0);
}
