//! Raw native handles for GPUI windows on Linux.
//!
//! GPUI 0.2.2's X11 window panics (`unimplemented!`) when asked for a raw
//! window or display handle, so they are only requested from Wayland windows.
use gpui::Window;
use raw_window_handle::{DisplayHandle, HasDisplayHandle, HasWindowHandle, WindowHandle};

/// True when GPUI runs this process on X11 rather than Wayland.
pub(crate) fn is_x11() -> bool {
    gpui::guess_compositor() == "X11"
}

fn is_wayland() -> bool {
    gpui::guess_compositor() == "Wayland"
}

pub(crate) fn window_handle(window: &Window) -> Option<WindowHandle<'_>> {
    is_wayland()
        .then(|| HasWindowHandle::window_handle(window).ok())
        .flatten()
}

pub(crate) fn display_handle(window: &Window) -> Option<DisplayHandle<'_>> {
    is_wayland()
        .then(|| HasDisplayHandle::display_handle(window).ok())
        .flatten()
}
