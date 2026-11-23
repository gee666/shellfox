use super::{Bridge, SemanticError};
use accesskit::TreeUpdate;
use accesskit_windows::SubclassingAdapter;
use gpui::{Bounds, Pixels, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{cell::Cell, ffi::c_void, rc::Rc, thread::ThreadId};
use windows::{
    Win32::{
        Foundation::{HANDLE, HWND},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetPropW, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
            RemovePropW, SW_SHOW, SetPropW, ShowWindowAsync,
        },
    },
    core::w,
};

mod end_session;
mod presentation;

const OWNER_PROPERTY: windows::core::PCWSTR = w!("Shellfox.Accessibility.OwnedPresentation");

// The cookie is a live allocation, not a numeric PID or a lookup by window title.
// Windows removes properties on destruction, so a recycled HWND cannot pass this check.
struct WindowOwner {
    hwnd: HWND,
    cookie: Box<u8>,
    native_thread: u32,
    rust_thread: ThreadId,
    alive: Cell<bool>,
}

fn borrowed_hwnd(window: &Window) -> Result<HWND, SemanticError> {
    let handle =
        HasWindowHandle::window_handle(window).map_err(|e| SemanticError::Native(e.to_string()))?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(SemanticError::Native("expected a Win32 GPUI window".into()));
    };
    Ok(HWND(handle.hwnd.get() as *mut c_void))
}

impl WindowOwner {
    fn attach(window: &Window) -> Result<Self, SemanticError> {
        let hwnd = borrowed_hwnd(window)?;
        let mut process = 0;
        let native_thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
        if native_thread == 0 || process != std::process::id() {
            return Err(SemanticError::Native(
                "GPUI window is not owned by this process".into(),
            ));
        }
        if unsafe { IsWindowVisible(hwnd) }.as_bool()
            || window.is_window_active()
            || unsafe { GetForegroundWindow() } == hwnd
        {
            return Err(SemanticError::Native(
                "attach before the window's first show/focus".into(),
            ));
        }
        if !unsafe { GetPropW(hwnd, OWNER_PROPERTY) }.0.is_null() {
            return Err(SemanticError::Native(
                "window already has a presentation owner".into(),
            ));
        }
        let owner = Self {
            hwnd,
            cookie: Box::new(0),
            native_thread,
            rust_thread: std::thread::current().id(),
            alive: Cell::new(true),
        };
        unsafe { SetPropW(hwnd, OWNER_PROPERTY, Some(HANDLE(owner.cookie_pointer()))) }
            .map_err(|e| SemanticError::Native(e.to_string()))?;
        Ok(owner)
    }

    fn cookie_pointer(&self) -> *mut c_void {
        (&*self.cookie as *const u8).cast_mut().cast()
    }
    fn cookie_matches(&self) -> bool {
        unsafe { GetPropW(self.hwnd, OWNER_PROPERTY) }.0 == self.cookie_pointer()
    }
    fn validate_native(&self) -> Result<(), SemanticError> {
        if !self.alive.get() || self.rust_thread != std::thread::current().id() {
            return Err(SemanticError::Native(
                "window owner was released or thread changed".into(),
            ));
        }
        let mut process = 0;
        let thread = unsafe { GetWindowThreadProcessId(self.hwnd, Some(&mut process)) };
        if !unsafe { IsWindow(Some(self.hwnd)) }.as_bool()
            || !self.cookie_matches()
            || thread != self.native_thread
            || process != std::process::id()
        {
            return Err(SemanticError::Native(
                "window lifetime/owner changed".into(),
            ));
        }
        Ok(())
    }
    fn validate(&self, window: &Window) -> Result<(), SemanticError> {
        if borrowed_hwnd(window)? != self.hwnd {
            return Err(SemanticError::Native("presentation window changed".into()));
        }
        self.validate_native()
    }
    fn validate_hidden_native(&self) -> Result<(), SemanticError> {
        self.validate_native()?;
        if unsafe { IsWindowVisible(self.hwnd) }.as_bool()
            || unsafe { GetForegroundWindow() } == self.hwnd
        {
            return Err(SemanticError::Native(
                "window was shown or activated before presentation".into(),
            ));
        }
        Ok(())
    }
    fn validate_hidden(&self, window: &Window) -> Result<(), SemanticError> {
        self.validate(window)?;
        if window.is_window_active() {
            return Err(SemanticError::Native(
                "window activated before presentation".into(),
            ));
        }
        self.validate_hidden_native()
    }
}
impl Drop for WindowOwner {
    fn drop(&mut self) {
        if self.rust_thread == std::thread::current().id() && self.cookie_matches() {
            let _ = unsafe { RemovePropW(self.hwnd, OWNER_PROPERTY) };
        }
    }
}

pub(super) struct Provider {
    adapter: SubclassingAdapter,
    owner: Rc<WindowOwner>,
    end_session: Option<end_session::EndSessionHook>,
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.owner.alive.set(false);
    }
}
impl Provider {
    pub(super) fn attach(window: &Window, bridge: Bridge) -> Result<Self, SemanticError> {
        let owner = Rc::new(WindowOwner::attach(window)?);
        let adapter = SubclassingAdapter::new(owner.hwnd, bridge.clone(), bridge);
        let end_session = end_session::EndSessionHook::install(owner.hwnd);
        Ok(Self {
            adapter,
            owner,
            end_session,
        })
    }
    pub(super) fn system_end_requested(&self) -> bool {
        self.end_session
            .as_ref()
            .is_some_and(end_session::EndSessionHook::requested)
    }

    pub(super) fn validate_window(&self, window: &Window) -> Result<(), SemanticError> {
        self.owner.validate(window)
    }
    pub(super) fn validate_hidden(&self, window: &Window) -> Result<(), SemanticError> {
        self.owner.validate_hidden(window)
    }
    pub(super) fn show_after_activation(&self, cx: &gpui::App) {
        let owner = Rc::downgrade(&self.owner);
        cx.foreground_executor()
            .spawn(async move {
                let Some(owner) = owner.upgrade() else {
                    return;
                };
                if owner.validate_native().is_ok()
                    && !unsafe { IsWindowVisible(owner.hwnd) }.as_bool()
                {
                    // GPUI applies the hidden Win32 placement on activation. A
                    // normal placement can retain SW_HIDE, unlike SW_MAXIMIZE.
                    let _ = unsafe { ShowWindowAsync(owner.hwnd, SW_SHOW) };
                }
            })
            .detach();
    }

    pub(super) fn queue_without_activation(
        &self,
        window: &Window,
        bounds: Bounds<Pixels>,
        cx: &gpui::App,
    ) -> Result<gpui::Task<Result<(), SemanticError>>, SemanticError> {
        self.owner.validate_hidden(window)?;
        let owner = Rc::downgrade(&self.owner);
        let handle = window.window_handle();
        let mut context = cx.to_async();
        Ok(cx.foreground_executor().spawn(async move {
            let owner = owner.upgrade().ok_or_else(|| SemanticError::Native("presentation owner released".into()))?;
            // This short borrow must finish BEFORE a native call can synchronously send WM_SIZE/WM_SHOWWINDOW.
            let scale = handle.update(&mut context, |_, window, _| -> Result<f32, SemanticError> {
                owner.validate_hidden(window)?;
                Ok(window.scale_factor())
            }).map_err(|e| SemanticError::Native(format!("presentation readiness failed: {e}")))??;
            let native_size = presentation::show(&owner, bounds, scale)?;
            // The native call has returned. Reacquire GPUI only to verify its REAL observed viewport.
            handle.update(&mut context, |_, window, _| -> Result<(), SemanticError> {
                owner.validate(window)?;
                let observed = gpui::Bounds::new(gpui::point(gpui::px(0.), gpui::px(0.)), window.viewport_size())
                    .to_device_pixels(window.scale_factor()).size;
                if observed != native_size {
                    return Err(SemanticError::Native(format!("GPUI missed native resize: viewport {observed:?}, native {native_size:?}")));
                }
                Ok(())
            }).map_err(|e| SemanticError::Native(format!("presentation completion failed: {e}")))??;
            Ok(())
        }))
    }
    pub(super) fn update_window(&mut self, _: &Window) {
        // AccessKit handles native focus, WM_GETOBJECT and WM_NCDESTROY.
    }
    pub(super) fn update(&mut self, update: TreeUpdate) {
        if let Some(events) = self.adapter.update_if_active(|| update) {
            events.raise();
        }
    }
}
