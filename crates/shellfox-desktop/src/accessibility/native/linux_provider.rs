use super::{Bridge, SemanticError};
use accesskit::{Rect, TreeUpdate};
use accesskit_unix::Adapter;
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(super) struct Provider {
    adapter: Adapter,
    x11: bool,
}

impl Provider {
    pub(super) fn attach(window: &Window, bridge: Bridge) -> Result<Self, SemanticError> {
        let handle = HasWindowHandle::window_handle(window)
            .map_err(|e| SemanticError::Native(e.to_string()))?;
        let x11 = match handle.as_raw() {
            RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => true,
            RawWindowHandle::Wayland(_) => false,
            _ => {
                return Err(SemanticError::Native(
                    "expected an X11 or Wayland GPUI window".into(),
                ));
            }
        };
        Ok(Self {
            adapter: Adapter::new(bridge.clone(), bridge.clone(), bridge),
            x11,
        })
    }

    pub(super) fn update_window(&mut self, window: &Window) {
        self.adapter
            .update_window_focus_state(window.is_window_active());
        if self.x11 {
            let scale = f64::from(window.scale_factor());
            let outer = window.bounds();
            let inner = window.inner_window_bounds().get_bounds();
            let rect = |b: gpui::Bounds<gpui::Pixels>| {
                Rect::new(
                    f64::from(f32::from(b.left())) * scale,
                    f64::from(f32::from(b.top())) * scale,
                    f64::from(f32::from(b.right())) * scale,
                    f64::from(f32::from(b.bottom())) * scale,
                )
            };
            self.adapter
                .set_root_window_bounds(rect(outer), rect(inner));
        }
        // Wayland does not expose a global window position. Do not invent one.
    }

    pub(super) fn update(&mut self, update: TreeUpdate) {
        self.adapter.update_if_active(|| update);
    }
}
