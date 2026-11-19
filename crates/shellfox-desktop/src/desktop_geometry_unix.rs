use crate::window_manager::state::{Monitor, Rect, WindowState, restore_bounds};
use gpui::{App, Bounds, Pixels, PlatformDisplay, Window, WindowBounds, WindowOptions};

fn monitor(display: &dyn PlatformDisplay) -> Monitor {
    let bounds = display.bounds();
    let x = pixels(bounds.origin.x);
    let y = pixels(bounds.origin.y);
    Monitor {
        id: display
            .uuid()
            .ok()
            .map_or_else(|| format!("{x}:{y}"), |id| id.to_string()),
        work_area: Rect {
            x,
            y,
            width: pixels(bounds.size.width),
            height: pixels(bounds.size.height),
        },
        physical_origin: (x, y),
        scale_factor: 1., // GPUI already supplies logical coordinates.
    }
}

fn pixels(value: Pixels) -> i32 {
    f32::from(value).round() as i32
}

pub(super) fn configure(cx: &App, saved: Option<WindowState>, options: &mut WindowOptions) {
    let Some(primary) = cx.primary_display() else {
        return;
    };
    let primary = monitor(primary.as_ref());
    let restored = (|| {
        #[cfg(target_os = "linux")]
        if gpui::guess_compositor() == "Wayland" {
            return crate::window_manager::state::restore_size_only(saved, &primary);
        }
        let monitors: Vec<_> = cx.displays().iter().map(|display| monitor(display.as_ref())).collect();
        restore_bounds(saved, &monitors, &primary)
    })();
    let Ok(restored) = restored else { return; };
    if let Ok(configured) = crate::window_manager::adapter::window_options(
        std::mem::take(options),
        restored,
        &primary,
    ) {
        *options = configured;
    }
}

pub(super) fn capture(window: &Window) -> Option<WindowState> {
    let (normal, maximized): (Bounds<Pixels>, _) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, true),
    };
    let origin = (pixels(normal.origin.x), pixels(normal.origin.y));
    #[cfg(target_os = "linux")]
    let origin = if gpui::guess_compositor() == "Wayland" { (0, 0) } else { origin };
    let state = WindowState {
        x: origin.0,
        y: origin.1,
        width: pixels(normal.size.width),
        height: pixels(normal.size.height),
        maximized,
    };
    state.valid().then_some(state)
}
