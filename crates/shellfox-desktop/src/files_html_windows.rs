use super::*;
use ::windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::ClientToScreen,
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GetClientRect, IsIconic, IsWindow, IsWindowVisible,
                SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos,
                WM_DPICHANGED, WM_NCDESTROY, WM_SHOWWINDOW, WM_SIZE, WM_WINDOWPOSCHANGED,
                WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::w,
};
use raw_window_handle::{HandleError, Win32WindowHandle, WindowHandle};
use std::{num::NonZeroIsize, path::PathBuf, rc::Weak};

// GPUI draws above its child HWNDs in DirectComposition. This owned popup is
// above the preview pane without changing the main window's rendering mode.
pub(super) struct Popup {
    anchor: Box<Anchor>,
}

struct Anchor {
    owner: Cell<HWND>,
    popup: HWND,
    bounds: Cell<Bounds<Pixels>>,
    showing: Cell<bool>,
}

impl Popup {
    fn new(owner: HWND, bounds: Bounds<Pixels>) -> anyhow::Result<Self> {
        // SAFETY: the UI thread owns this still-live GPUI parent. STATIC is a
        // registered system window class and receives no creation payload.
        let popup = unsafe {
            anyhow::ensure!(IsWindow(Some(owner)).as_bool(), "Preview owner closed");
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!(""),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                Some(owner),
                None,
                None,
                None,
            )?
        };
        let this = Self {
            anchor: Box::new(Anchor {
                owner: Cell::new(owner),
                popup,
                bounds: Cell::new(bounds),
                showing: Cell::new(false),
            }),
        };
        // SAFETY: the allocation stays pinned at this address. Drop removes the
        // subclass before freeing it, and WM_NCDESTROY removes it on owner teardown.
        anyhow::ensure!(
            unsafe { SetWindowSubclass(owner, Some(follow_owner), this.id(), this.id()) }.as_bool(),
            "Could not follow the preview owner"
        );
        this.anchor.place();
        Ok(this)
    }

    fn id(&self) -> usize {
        self.anchor.as_ref() as *const Anchor as usize
    }

    pub(super) fn resize(&self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        self.anchor.bounds.set(bounds);
        self.anchor.place();
        child_bounds(bounds)
    }

    pub(super) fn show(&self) {
        self.anchor.showing.set(true);
        self.anchor.place();
    }
}

impl HasWindowHandle for Popup {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.anchor.popup;
        // SAFETY: this only checks the handle. The owning UI thread has not
        // released the popup wrapper during the following borrow.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err(HandleError::Unavailable);
        }
        let value = NonZeroIsize::new(hwnd.0 as isize).ok_or(HandleError::Unavailable)?;
        // SAFETY: the borrow cannot outlive the owning Popup.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(value)))
        })
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        // SAFETY: both operations run on the owning UI thread before Anchor is freed.
        unsafe {
            let owner = self.anchor.owner.get();
            if !owner.is_invalid() {
                let _ = RemoveWindowSubclass(owner, Some(follow_owner), self.id());
            }
            if IsWindow(Some(self.anchor.popup)).as_bool() {
                let _ = DestroyWindow(self.anchor.popup);
            }
        }
    }
}

impl Anchor {
    fn place(&self) {
        let owner = self.owner.get();
        let popup = self.popup;
        if owner.is_invalid() {
            return;
        }
        // SAFETY: Win32 fills these stack rectangles/points, and all calls stay
        // on the UI thread that owns the main window and popup.
        unsafe {
            let mut client = RECT::default();
            let mut start = POINT::default();
            let bounds = self.bounds.get();
            let scale = GetDpiForWindow(owner).max(96) as f32 / 96.;
            let pixels = |value: Pixels| (f32::from(value) * scale).round() as i32;
            let x = pixels(bounds.origin.x);
            let y = pixels(bounds.origin.y);
            start.x = x;
            start.y = y;
            if GetClientRect(owner, &mut client).is_err()
                || !ClientToScreen(owner, &mut start).as_bool()
            {
                return;
            }
            let width = pixels(bounds.size.width).min(client.right.saturating_sub(x));
            let height = pixels(bounds.size.height).min(client.bottom.saturating_sub(y));
            let show = self.showing.get()
                && !IsIconic(owner).as_bool()
                && IsWindowVisible(owner).as_bool()
                && x >= 0
                && y >= 0
                && width > 0
                && height > 0;
            let _ = SetWindowPos(
                popup,
                None,
                start.x,
                start.y,
                width.max(1),
                height.max(1),
                SWP_NOACTIVATE | SWP_NOZORDER | if show { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW },
            );
        }
    }
}

unsafe extern "system" fn follow_owner(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: user data is the live Box<Anchor>, held until the subclass is removed.
    let anchor = unsafe { &*(data as *const Anchor) };
    match message {
        WM_NCDESTROY => {
            anchor.owner.set(HWND::default());
            // SAFETY: remove this exact subclass before forwarding owner destruction.
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(follow_owner), id) };
        }
        WM_WINDOWPOSCHANGED | WM_SIZE | WM_DPICHANGED | WM_SHOWWINDOW => anchor.place(),
        _ => {}
    }
    // SAFETY: forward the unmodified message to the next subclass.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub(super) fn contain_frame_navigation(view: &WebView) -> anyhow::Result<()> {
    use ::windows::core::PWSTR;
    use webview2_com::{NavigationStartingEventHandler, take_pwstr};
    use wry::WebViewExtWindows;

    let initial = Cell::new(true);
    let handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            // SAFETY: COM supplies the current event arguments and allocates the
            // URI for the caller. take_pwstr frees that allocation.
            unsafe {
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                args.SetCancel(uri != sandbox::FRAME || !initial.replace(false))?;
            }
        }
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: the controller and its event handler live on their owning STA.
    // This handler holds no view reference, so destroying the controller releases it.
    unsafe {
        view.webview()
            .add_FrameNavigationStarting(&handler, &mut token)?;
    }
    Ok(())
}

fn child_bounds(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: gpui::Point::default(),
        size: bounds.size,
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
    let owner = HasWindowHandle::window_handle(window)
        .ok()
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
            _ => None,
        });
    // WebView2 dispatches Win32 messages while waiting for COM. The UI thread
    // must not be holding a borrowed GPUI application or window during startup.
    cx.spawn(async move |cx| {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        if !inner.borrow().is_current(&bytes) {
            inner.borrow_mut().creating = false;
            let _ = cx.update(|cx| cx.refresh_windows());
            return;
        }
        let result = (|| {
            let owner = owner.ok_or_else(|| anyhow::anyhow!("No Windows preview parent"))?;
            let popup = Popup::new(owner, bounds)?;
            let profile = tempfile::Builder::new()
                .prefix("html-preview-")
                .tempdir_in(profile_parent)?;
            let view = create(&popup, child_bounds(bounds), bytes.clone(), profile.path())?;
            Ok(NativeView {
                view,
                popup,
                _profile: profile,
            })
        })();
        if Rc::strong_count(&inner) > 1 {
            let _ = cx.update(|cx| publish(&mut inner.borrow_mut(), bounds, &bytes, result, cx));
        }
    })
    .detach();
}
