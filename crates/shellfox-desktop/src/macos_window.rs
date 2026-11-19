use gpui::Window;
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(crate) fn owner(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // GPUI lends a live NSView on the main thread; window() retains its owner.
    unsafe { handle.ns_view.cast::<NSView>().as_ref() }.window()
}

pub(crate) fn with_window<T>(window: &Window, read: impl FnOnce(&NSWindow) -> T) -> Option<T> {
    owner(window).as_deref().map(read)
}
