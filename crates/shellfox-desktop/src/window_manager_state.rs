//! Where a saved window opens. Bounds are GPUI logical pixels; the saved
//! record is relative to its display, see [`WindowState`].
use gpui::{Bounds, Pixels, Size, point, px, size};
use std::time::Duration;
#[cfg(test)]
#[path = "window_manager_state_tests.rs"]
mod tests;

pub use shellfox_core::window_state::{
    DEFAULT_HEIGHT, DEFAULT_WIDTH, MIN_HEIGHT, MIN_WIDTH, WindowState, client_size,
};
pub const SAVE_DELAY: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub uuid: Option<String>,
    /// Global GPUI bounds of the display.
    pub bounds: Bounds<Pixels>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// Index into the screens passed to [`place`].
    pub screen: usize,
    pub bounds: Bounds<Pixels>,
    pub maximized: bool,
}

pub fn min_size() -> Size<Pixels> {
    let (width, height) = client_size(MIN_WIDTH, MIN_HEIGHT);
    size(px(width as f32), px(height as f32))
}

/// Restores on the saved display, or on the primary display when it is gone.
/// The window keeps its size where it fits and is moved fully on-screen.
pub fn place(saved: Option<&WindowState>, screens: &[Screen], primary: usize) -> Option<Placement> {
    let screen = saved
        .and_then(|saved| saved.display.as_ref())
        .and_then(|id| screens.iter().position(|screen| screen.uuid.as_ref() == Some(id)))
        .unwrap_or(primary);
    let area = screens.get(screen)?.bounds;
    let (default_width, default_height) = client_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    let (width, height) = saved.map_or((default_width, default_height), |saved| {
        (saved.width, saved.height)
    });
    let min = min_size();
    let width = (width as f32).min(area.size.width.into()).max(min.width.into());
    let height = (height as f32).min(area.size.height.into()).max(min.height.into());
    let free_x = (f32::from(area.size.width) - width).max(0.);
    let free_y = (f32::from(area.size.height) - height).max(0.);
    let (x, y) = saved.map_or((free_x / 2., free_y / 2.), |saved| {
        (saved.x as f32, saved.y as f32)
    });
    Some(Placement {
        screen,
        bounds: Bounds::new(
            point(
                area.origin.x + px(x.clamp(0., free_x)),
                area.origin.y + px(y.clamp(0., free_y)),
            ),
            size(px(width), px(height)),
        ),
        maximized: saved.is_some_and(|saved| saved.maximized),
    })
}

/// The record for a window's normal bounds on a display.
pub fn record(
    normal: Bounds<Pixels>,
    maximized: bool,
    display: Option<&Screen>,
) -> Option<WindowState> {
    let origin = display.map_or(point(px(0.), px(0.)), |display| display.bounds.origin);
    let state = WindowState {
        x: f32::from(normal.origin.x - origin.x).round() as i32,
        y: f32::from(normal.origin.y - origin.y).round() as i32,
        width: f32::from(normal.size.width).round() as i32,
        height: f32::from(normal.size.height).round() as i32,
        maximized,
        display: display.and_then(|display| display.uuid.clone()),
    };
    state.valid().then_some(state)
}
