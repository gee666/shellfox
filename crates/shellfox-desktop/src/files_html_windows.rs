use super::*;
use raw_window_handle::{HandleError, WindowHandle};
use std::{path::PathBuf, rc::Weak};
use ::windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::IsWindow};

struct Parent(raw_window_handle::Win32WindowHandle);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = HWND(self.0.hwnd.get() as *mut _);
        // SAFETY: IsWindow checks the handle without dereferencing it. This is the
        // same UI thread that owns the parent and creates the child webview.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err(HandleError::Unavailable);
        }
        // SAFETY: self contains the verified live HWND during this borrow.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(self.0)) })
    }
}

pub(super) fn schedule(
    window: &Window,
    weak: Weak<RefCell<State>>,
    bounds: Bounds<Pixels>,
    bytes: Arc<[u8]>,
    profile_parent: PathBuf,
    cx: &mut gpui::App,
) {
    let parent = HasWindowHandle::window_handle(window).ok().and_then(|handle| match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(Parent(handle)),
        _ => None,
    });
    // WebView2 initialization dispatches Win32 messages while waiting for COM.
    // Schedule it on the UI thread with no borrowed GPUI application or window.
    cx.spawn(async move |cx| {
        let Some(inner) = weak.upgrade() else { return; };
        if !inner.borrow().is_current(&bytes) {
            inner.borrow_mut().creating = false;
            let _ = cx.update(|cx| cx.refresh_windows());
            return;
        }
        let result = (|| {
            let parent = parent.ok_or_else(|| anyhow::anyhow!("No Windows preview parent"))?;
            let profile = tempfile::Builder::new().prefix("html-preview-").tempdir_in(profile_parent)?;
            create(&parent, bounds, bytes.clone(), profile)
        })();
        if Rc::strong_count(&inner) > 1 {
            let _ = cx.update(|cx| publish(&mut inner.borrow_mut(), bounds, &bytes, result, cx));
        }
    }).detach();
}
