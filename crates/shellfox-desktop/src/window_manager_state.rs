//! Where a saved window opens. Bounds are GPUI logical pixels; the saved
//! record is relative to its display, see [`WindowState`].
use gpui::{Bounds, Pixels, Point, Size, point, px, size};
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
    /// The part not covered by the taskbar, menu bar or dock, relative to `bounds`.
    pub work: Bounds<Pixels>,
    /// Windows places windows in workspace coordinates, which start at the work
    /// area rather than the monitor. Elsewhere this is zero.
    pub workspace: Point<Pixels>,
}

impl Screen {
    pub fn full(uuid: Option<String>, bounds: Bounds<Pixels>) -> Self {
        Self {
            uuid,
            work: Bounds::new(Point::default(), bounds.size),
            workspace: Point::default(),
            bounds,
        }
    }
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
/// The window keeps its size where it fits and is moved fully into the work area.
pub fn place(saved: Option<&WindowState>, screens: &[Screen], primary: usize) -> Option<Placement> {
    let screen = saved
        .and_then(|saved| saved.display.as_ref())
        .and_then(|id| screens.iter().position(|screen| screen.uuid.as_ref() == Some(id)))
        .unwrap_or(primary);
    let display = screens.get(screen)?;
    let work = display.work;
    let (default_width, default_height) = client_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    let (width, height) = saved.map_or((default_width, default_height), |saved| {
        (saved.width, saved.height)
    });
    let min = min_size();
    let width = (width as f32).min(work.size.width.into()).max(min.width.into());
    let height = (height as f32).min(work.size.height.into()).max(min.height.into());
    let free_x = (f32::from(work.size.width) - width).max(0.);
    let free_y = (f32::from(work.size.height) - height).max(0.);
    let (x, y) = saved.map_or(((free_x / 2.).round(), (free_y / 2.).round()), |saved| {
        (
            saved.x as f32 - f32::from(work.origin.x),
            saved.y as f32 - f32::from(work.origin.y),
        )
    });
    let origin = display.bounds.origin + work.origin - display.workspace;
    Some(Placement {
        screen,
        bounds: Bounds::new(
            point(
                origin.x + px(x.clamp(0., free_x)),
                origin.y + px(y.clamp(0., free_y)),
            ),
            size(px(width), px(height)),
        ),
        maximized: saved.is_some_and(|saved| saved.maximized),
    })
}

/// The record for a window's normal bounds, in GPUI window coordinates, on a display.
pub fn record(
    normal: Bounds<Pixels>,
    maximized: bool,
    display: Option<&Screen>,
) -> Option<WindowState> {
    let origin = display.map_or(Point::default(), |display| {
        display.bounds.origin - display.workspace
    });
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
