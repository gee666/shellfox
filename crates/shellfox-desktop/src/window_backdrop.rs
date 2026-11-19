use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DwmSetWindowAttribute},
};

pub(super) fn apply(window: &Window) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    // Electron uses the Windows 11 main-window Mica material. Keep native
    // caption colors so DWM applies the system's active and inactive styles.
    // Older Windows versions reject this attribute and keep their native frame.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            std::ptr::from_ref(&DWMSBT_MAINWINDOW).cast(),
            std::mem::size_of_val(&DWMSBT_MAINWINDOW) as u32,
        )
    };
}
