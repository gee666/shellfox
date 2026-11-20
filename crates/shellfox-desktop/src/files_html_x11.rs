use gtk::glib::prelude::Cast;
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, WindowHandle, XlibWindowHandle,
};

pub(super) struct Parent<'a>(WindowHandle<'a>);

impl<'a> Parent<'a> {
    pub fn from_gpui(window: &'a impl HasWindowHandle) -> anyhow::Result<Self> {
        // GPUI and GTK use separate X11 connections, but share the XID namespace.
        // Wry expects GTK's X11 display and cannot parent to a Wayland handle.
        if !gtk::is_initialized() {
            gtk::gdk::set_allowed_backends("x11");
        }
        gtk::init().map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let x11 = gtk::gdk::Display::default()
            .is_some_and(|display| display.downcast_ref::<gdkx11::X11Display>().is_some());
        anyhow::ensure!(x11, "HTML preview requires GTK's X11 display");
        Ok(Self(window.window_handle()?))
    }
}

impl HasWindowHandle for Parent<'_> {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        match self.0.as_raw() {
            RawWindowHandle::Xlib(_) => Ok(self.0),
            RawWindowHandle::Xcb(handle) => {
                let xlib = XlibWindowHandle::new(handle.window.get().into());
                // SAFETY: Xlib and XCB use the same XID. The original GPUI window
                // is still borrowed by self.0 for the lifetime of this handle.
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xlib(xlib)) })
            }
            _ => Err(HandleError::Unavailable),
        }
    }
}
