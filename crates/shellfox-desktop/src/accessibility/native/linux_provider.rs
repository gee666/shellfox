use super::{Bridge, SemanticError};
use accesskit::{Rect, TreeUpdate};
use accesskit_unix::Adapter;
use gpui::Window;

pub(super) struct Provider {
    adapter: Adapter,
    x11: bool,
}

impl Provider {
    pub(super) fn attach(_window: &Window, bridge: Bridge) -> Result<Self, SemanticError> {
        // AccessKit's Unix adapter needs no window handle, only the backend kind.
        let x11 = crate::native_window::is_x11();
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
