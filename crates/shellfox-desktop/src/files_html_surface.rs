use gpui::{Bounds, Pixels, Window, canvas, prelude::*};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};
use wry::{NewWindowResponse, PermissionResponse, WebView, WebViewBuilder, dpi};
#[path = "files_html_sandbox.rs"]
mod sandbox;
#[cfg(windows)]
#[path = "files_html_windows.rs"]
mod windows;
#[cfg(target_os = "linux")]
#[path = "files_html_x11.rs"]
mod x11;

pub(crate) struct Surface {
    // Own only the preview shown in this window. Retained editors don't own native children.
    inner: Rc<RefCell<State>>,
    used: Cell<bool>,
    #[cfg(windows)]
    profile_parent: std::path::PathBuf,
}

#[derive(Default)]
struct State {
    unavailable: bool,
    creating: bool,
    requested: Option<Arc<[u8]>>,
    view: Option<(NativeView, Bounds<Pixels>)>,
    #[cfg(target_os = "linux")]
    pump: Option<gpui::Task<()>>,
}

struct NativeView {
    view: WebView,
    #[cfg(windows)]
    popup: windows::Popup,
    #[cfg(windows)]
    _profile: tempfile::TempDir,
}

impl NativeView {
    fn resize(&self, bounds: Bounds<Pixels>) -> wry::Result<()> {
        #[cfg(windows)]
        let bounds = self.popup.resize(bounds);
        self.view.set_bounds(rect(bounds))
    }

    fn show(&self) -> wry::Result<()> {
        self.view.set_visible(true)?;
        #[cfg(windows)]
        self.popup.show();
        Ok(())
    }
}

impl Surface {
    pub fn new(run_dir: &std::path::Path) -> Self {
        let _ = run_dir;
        Self {
            inner: Rc::default(),
            used: Cell::default(),
            #[cfg(windows)]
            profile_parent: run_dir.to_owned(),
        }
    }

    pub fn available(&self, window: &Window) -> bool {
        !self.inner.borrow().unavailable
            && HasWindowHandle::window_handle(window)
                .is_ok_and(|handle| supports_embedding(handle.as_raw()))
    }

    pub fn begin_frame(&self) {
        self.used.set(false);
    }

    pub fn end_frame(&self, covered: bool) {
        if covered || !self.used.get() {
            let mut state = self.inner.borrow_mut();
            state.requested = None;
            state.remove_view();
        }
    }

    pub fn element(&self, bytes: Arc<[u8]>) -> impl IntoElement {
        self.used.set(true);
        {
            let mut state = self.inner.borrow_mut();
            if !state.is_current(&bytes) {
                state.remove_view();
                state.requested = Some(bytes.clone());
            }
        }
        let weak = Rc::downgrade(&self.inner);
        #[cfg(windows)]
        let profile_parent = self.profile_parent.clone();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, cx| {
                if bounds.size.width <= gpui::px(0.) || bounds.size.height <= gpui::px(0.) {
                    if let Some(inner) = weak.upgrade() {
                        inner.borrow_mut().remove_view();
                    }
                    return;
                }
                window.defer(cx, move |window, cx| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    {
                        let mut state = inner.borrow_mut();
                        if !state.is_current(&bytes) || state.unavailable || state.creating {
                            return;
                        }
                        if let Some((view, previous)) = &mut state.view {
                            if *previous == bounds || view.resize(bounds).is_ok() {
                                *previous = bounds;
                                return;
                            }
                            state.remove_view();
                        }
                        state.creating = true;
                    }
                    #[cfg(windows)]
                    windows::schedule(
                        window,
                        Rc::downgrade(&inner),
                        bounds,
                        bytes,
                        profile_parent,
                        cx,
                    );
                    #[cfg(not(windows))]
                    {
                        let result =
                            create(window, bounds, bytes.clone()).map(|view| NativeView { view });
                        publish(&mut inner.borrow_mut(), bounds, &bytes, result, cx);
                    }
                });
            },
        )
        .flex_1()
        .min_h_0()
        .w_full()
    }
}

fn supports_embedding(handle: RawWindowHandle) -> bool {
    // GPUI has no GTK container or equivalent cross-toolkit parenting on Wayland.
    match handle {
        RawWindowHandle::Win32(_) => cfg!(windows),
        RawWindowHandle::AppKit(_) => cfg!(target_os = "macos"),
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => cfg!(target_os = "linux"),
        _ => false,
    }
}

#[cfg(test)]
#[test]
fn wayland_handles_fall_back_to_source() {
    let handle = raw_window_handle::WaylandWindowHandle::new(std::ptr::NonNull::dangling());
    assert!(!supports_embedding(RawWindowHandle::Wayland(handle)));
}

impl State {
    fn is_current(&self, bytes: &Arc<[u8]>) -> bool {
        self.requested
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, bytes))
    }

    fn remove_view(&mut self) {
        self.view = None;
        #[cfg(target_os = "linux")]
        {
            self.pump = None;
        }
    }
}

fn publish(
    state: &mut State,
    bounds: Bounds<Pixels>,
    bytes: &Arc<[u8]>,
    result: anyhow::Result<NativeView>,
    cx: &mut gpui::App,
) {
    state.creating = false;
    if !state.is_current(bytes) {
        cx.refresh_windows();
        return;
    }
    match result.and_then(|view| {
        view.show()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(view)
    }) {
        Ok(view) => {
            state.view = Some((view, bounds));
            #[cfg(target_os = "linux")]
            {
                state.pump = Some(pump_gtk(cx));
            }
            // The owner may have resized while the native controller initialized.
            cx.refresh_windows();
        }
        Err(error) => {
            eprintln!("HTML preview unavailable: {error:#}");
            state.unavailable = true;
            cx.refresh_windows();
        }
    }
}

fn rect(bounds: Bounds<Pixels>) -> wry::Rect {
    wry::Rect {
        position: dpi::LogicalPosition::new(f32::from(bounds.origin.x), f32::from(bounds.origin.y))
            .into(),
        size: dpi::LogicalSize::new(f32::from(bounds.size.width), f32::from(bounds.size.height))
            .into(),
    }
}

fn create(
    window: &impl HasWindowHandle,
    bounds: Bounds<Pixels>,
    bytes: Arc<[u8]>,
    #[cfg(windows)] profile: &std::path::Path,
) -> anyhow::Result<WebView> {
    #[cfg(target_os = "linux")]
    let parent = x11::Parent::from_gpui(window)?;
    #[cfg(target_os = "linux")]
    let window = &parent;
    #[cfg(windows)]
    let mut context = wry::WebContext::new(Some(profile.to_owned()));
    #[cfg(windows)]
    let builder = WebViewBuilder::new_with_web_context(&mut context);
    #[cfg(not(windows))]
    let builder = WebViewBuilder::new();
    let navigation = sandbox::Navigation::default();
    let builder = builder
        .with_incognito(true)
        .with_bounds(rect(bounds))
        .with_focused(false)
        .with_visible(false)
        .with_hotkeys_zoom(false)
        .with_navigation_handler(move |url| navigation.allow(&url))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_permission_handler(|_| PermissionResponse::Deny)
        .with_custom_protocol(sandbox::SCHEME.to_owned(), move |_, request| {
            sandbox::respond(&request, &bytes)
        });
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder
            .with_browser_accelerator_keys(false)
            .with_default_context_menus(false)
            .with_drag_drop_handler(|_| true)
    };
    let view = builder
        .build_as_child(window)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    #[cfg(windows)]
    windows::contain_frame_navigation(&view)?;
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::{SettingsExt, WebViewExt};
        use wry::WebViewExtUnix;
        let settings = view
            .webview()
            .settings()
            .ok_or_else(|| anyhow::anyhow!("No WebKit settings"))?;
        settings.set_enable_dns_prefetching(false);
        settings.set_enable_webrtc(false);
        settings.set_enable_media_stream(false);
    }
    view.load_url(sandbox::ENTRY)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(view)
}

#[cfg(target_os = "linux")]
fn pump_gtk(cx: &mut gpui::App) -> gpui::Task<()> {
    let executor = cx.background_executor().clone();
    cx.spawn(async move |_| {
        let context = gtk::glib::MainContext::default();
        loop {
            for _ in 0..16 {
                if !context.iteration(false) {
                    break;
                }
            }
            executor.timer(std::time::Duration::from_millis(16)).await;
        }
    })
}
