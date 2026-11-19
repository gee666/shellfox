use gpui::{Bounds, Pixels, Window, canvas, prelude::*};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{cell::{Cell, RefCell}, rc::Rc, sync::Arc};
use wry::{NewWindowResponse, PermissionResponse, WebView, WebViewBuilder, dpi, http};
#[cfg(windows)]
#[path = "files_html_windows.rs"]
mod windows;

const SCHEME: &str = "shellfox-preview";
const DOCUMENT: &str = "shellfox-preview://document/";
#[cfg(windows)]
const ENTRY: &str = "http://shellfox-preview.document/";
#[cfg(not(windows))]
const ENTRY: &str = DOCUMENT;

// Match src/shared/file-preview-url.ts. The HTTP sandbox keeps the document opaque;
// a CSP meta tag cannot set sandbox flags.
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' data: blob:; style-src 'unsafe-inline' data: blob:; img-src data: blob:; font-src data: blob:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; sandbox allow-scripts";

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
    _profile: tempfile::TempDir,
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
        !self.inner.borrow().unavailable && HasWindowHandle::window_handle(window).is_ok_and(|handle| {
            // GPUI exposes a native X11 window, not a GTK container. There is no
            // equivalent cross-toolkit child window to parent to on Wayland.
            matches!(handle.as_raw(), RawWindowHandle::Win32(_) | RawWindowHandle::AppKit(_) | RawWindowHandle::Xlib(_))
        })
    }

    pub fn begin_frame(&self) { self.used.set(false); }

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
        canvas(|_, _, _| (), move |bounds, (), window, cx| {
            if bounds.size.width <= gpui::px(0.) || bounds.size.height <= gpui::px(0.) {
                if let Some(inner) = weak.upgrade() { inner.borrow_mut().remove_view(); }
                return;
            }
            window.defer(cx, move |window, cx| {
                let Some(inner) = weak.upgrade() else { return; };
                {
                    let mut state = inner.borrow_mut();
                    if !state.is_current(&bytes) || state.unavailable || state.creating { return; }
                    if let Some((view, previous)) = &mut state.view {
                        if *previous == bounds || view.view.set_bounds(rect(bounds)).is_ok() {
                            *previous = bounds;
                            return;
                        }
                        state.remove_view();
                    }
                    state.creating = true;
                }
                #[cfg(windows)]
                windows::schedule(window, Rc::downgrade(&inner), bounds, bytes, profile_parent, cx);
                #[cfg(not(windows))]
                {
                    let result = create(window, bounds, bytes.clone());
                    publish(&mut inner.borrow_mut(), bounds, &bytes, result, cx);
                }
            });
        }).flex_1().min_h_0().w_full()
    }
}

impl State {
    fn is_current(&self, bytes: &Arc<[u8]>) -> bool {
        self.requested.as_ref().is_some_and(|old| Arc::ptr_eq(old, bytes))
    }

    fn remove_view(&mut self) {
        self.view = None;
        #[cfg(target_os = "linux")]
        { self.pump = None; }
    }
}

fn publish(state: &mut State, bounds: Bounds<Pixels>, bytes: &Arc<[u8]>, result: anyhow::Result<NativeView>, cx: &mut gpui::App) {
    state.creating = false;
    if !state.is_current(bytes) {
        cx.refresh_windows();
        return;
    }
    match result.and_then(|view| {
        view.view.set_visible(true).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(view)
    }) {
        Ok(view) => {
            state.view = Some((view, bounds));
            #[cfg(target_os = "linux")]
            { state.pump = Some(pump_gtk(cx)); }
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
        position: dpi::LogicalPosition::new(f32::from(bounds.origin.x), f32::from(bounds.origin.y)).into(),
        size: dpi::LogicalSize::new(f32::from(bounds.size.width), f32::from(bounds.size.height)).into(),
    }
}

fn document(request: &http::Request<Vec<u8>>, bytes: &[u8]) -> http::Response<std::borrow::Cow<'static, [u8]>> {
    if request.method() != http::Method::GET || request.uri() != DOCUMENT {
        return http::Response::builder().status(404).body(std::borrow::Cow::Borrowed(&[][..])).expect("static response");
    }
    http::Response::builder()
        .header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(http::header::CONTENT_SECURITY_POLICY, CSP)
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(http::header::REFERRER_POLICY, "no-referrer")
        .header(http::header::CACHE_CONTROL, "no-store")
        .header("X-DNS-Prefetch-Control", "off")
        .body(std::borrow::Cow::Owned(bytes.to_vec()))
        .expect("static response headers")
}

fn create(
    window: &impl HasWindowHandle,
    bounds: Bounds<Pixels>,
    bytes: Arc<[u8]>,
    #[cfg(windows)] profile: tempfile::TempDir,
) -> anyhow::Result<NativeView> {
    #[cfg(target_os = "linux")]
    gtk::init().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    #[cfg(windows)]
    let mut context = wry::WebContext::new(Some(profile.path().to_owned()));
    #[cfg(windows)]
    let builder = WebViewBuilder::new_with_web_context(&mut context);
    #[cfg(not(windows))]
    let builder = WebViewBuilder::new();
    let builder = builder
        .with_incognito(true)
        .with_bounds(rect(bounds))
        .with_focused(false)
        .with_visible(false)
        .with_hotkeys_zoom(false)
        .with_navigation_handler(|url| url == ENTRY || url == DOCUMENT)
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_permission_handler(|_| PermissionResponse::Deny)
        .with_custom_protocol(SCHEME.to_owned(), move |_, request| document(&request, &bytes));
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_browser_accelerator_keys(false).with_default_context_menus(false)
    };
    let view = builder.build_as_child(window).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::prelude::{SettingsExt, WebViewExt};
        use wry::WebViewExtUnix;
        let settings = view.webview().settings().ok_or_else(|| anyhow::anyhow!("No WebKit settings"))?;
        settings.set_enable_dns_prefetching(false);
    }
    view.load_url(ENTRY).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(NativeView {
        view,
        #[cfg(windows)]
        _profile: profile,
    })
}

#[cfg(target_os = "linux")]
fn pump_gtk(cx: &mut gpui::App) -> gpui::Task<()> {
    let executor = cx.background_executor().clone();
    cx.spawn(async move |_| {
        let context = gtk::glib::MainContext::default();
        loop {
            for _ in 0..16 {
                if !context.iteration(false) { break; }
            }
            executor.timer(std::time::Duration::from_millis(16)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_document_is_served_with_an_opaque_script_sandbox() {
        for (method, url) in [
            ("GET", DOCUMENT), ("POST", DOCUMENT), ("GET", "shellfox-preview://document/file"),
            ("GET", "shellfox-preview://wrong/"), ("GET", "shellfox-preview://document/?extra=1"),
        ] {
            let request = http::Request::builder().uri(url).method(method).body(Vec::new()).unwrap();
            let response = document(&request, b"preview");
            if method == "GET" && url == DOCUMENT {
                assert_eq!(response.status(), 200);
                assert_eq!(response.headers()[http::header::CONTENT_SECURITY_POLICY], CSP);
                assert_eq!(&**response.body(), b"preview");
            } else {
                assert_eq!(response.status(), 404);
                assert!(response.body().is_empty());
            }
        }
    }
}
