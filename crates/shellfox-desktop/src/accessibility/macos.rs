use super::{Bridge, SemanticError};
use accesskit::TreeUpdate;
use accesskit_macos::SubclassingAdapter;
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(super) struct Provider(SubclassingAdapter);

impl Provider {
    pub(super) fn attach(window: &Window, bridge: Bridge) -> Result<Self, SemanticError> {
        let handle = HasWindowHandle::window_handle(window)
            .map_err(|e| SemanticError::Native(e.to_string()))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err(SemanticError::Native(
                "expected an AppKit GPUI window".into(),
            ));
        };
        // SAFETY: GPUI's borrowed Window supplies a live NSView on the main thread.
        // AccessKit retains it, restores its class on Drop, and does not own the NSWindow.
        // The caller must create hidden/unfocused with automatic window tabbing disabled.
        let adapter =
            unsafe { SubclassingAdapter::new(handle.ns_view.as_ptr(), bridge.clone(), bridge) };
        Ok(Self(adapter))
    }

    pub(super) fn update_window(&mut self, window: &Window) {
        if let Some(events) = self.0.update_view_focus_state(window.is_window_active()) {
            events.raise();
        }
    }

    pub(super) fn update(&mut self, update: TreeUpdate) {
        if let Some(events) = self.0.update_if_active(|| update) {
            events.raise();
        }
    }
}
