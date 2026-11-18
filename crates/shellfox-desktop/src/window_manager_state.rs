//! Electron window-state.ts policy in logical pixels. Persist the five original
//! fields per device/window; never persist global terminal selection here.
use anyhow::{Result, ensure};
use std::time::{Duration, Instant};
#[cfg(test)]
#[path = "window_manager_state_tests.rs"]
mod tests;

pub const DEFAULT_WIDTH: i32 = 1220;
pub const DEFAULT_HEIGHT: i32 = 820;
pub const MIN_WIDTH: i32 = 850;
pub const MIN_HEIGHT: i32 = 600;
pub const SAVE_DELAY: Duration = Duration::from_millis(400);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}
impl WindowState {
    pub fn valid(self) -> bool {
        (-100_000..=100_000).contains(&self.x)
            && (-100_000..=100_000).contains(&self.y)
            && (MIN_WIDTH..=20_000).contains(&self.width)
            && (MIN_HEIGHT..=20_000).contains(&self.height)
    }
}
#[derive(Clone, Debug)]
pub struct Monitor {
    pub id: String,
    /// GPUI/Electron logical work area, not physical desktop coordinates.
    pub work_area: Rect,
    pub physical_origin: (i32, i32),
    pub scale_factor: f64,
}
impl Monitor {
    pub fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 256
            && self.scale_factor.is_finite()
            && (0.25..=8.).contains(&self.scale_factor)
            && self.work_area.width > 0
            && self.work_area.height > 0
            && self.work_area.width <= 20_000
            && self.work_area.height <= 20_000
            && self.work_area.x.abs_diff(0) <= 100_000
            && self.work_area.y.abs_diff(0) <= 100_000
    }
    pub fn to_physical(&self, logical: Rect) -> Result<Rect> {
        ensure!(self.valid(), "Invalid monitor geometry or scale.");
        Ok(Rect {
            x: rounded(
                f64::from(self.physical_origin.0)
                    + (f64::from(logical.x) - f64::from(self.work_area.x)) * self.scale_factor,
            )?,
            y: rounded(
                f64::from(self.physical_origin.1)
                    + (f64::from(logical.y) - f64::from(self.work_area.y)) * self.scale_factor,
            )?,
            width: rounded(f64::from(logical.width) * self.scale_factor)?,
            height: rounded(f64::from(logical.height) * self.scale_factor)?,
        })
    }
    fn to_logical(&self, physical: Rect) -> Result<Rect> {
        ensure!(self.valid(), "Invalid monitor geometry or scale.");
        Ok(Rect {
            x: rounded(
                f64::from(self.work_area.x)
                    + (f64::from(physical.x) - f64::from(self.physical_origin.0))
                        / self.scale_factor,
            )?,
            y: rounded(
                f64::from(self.work_area.y)
                    + (f64::from(physical.y) - f64::from(self.physical_origin.1))
                        / self.scale_factor,
            )?,
            width: rounded(f64::from(physical.width) / self.scale_factor)?,
            height: rounded(f64::from(physical.height) / self.scale_factor)?,
        })
    }
}
fn rounded(value: f64) -> Result<i32> {
    let value = value.round();
    ensure!(
        value.is_finite() && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX),
        "Window coordinate overflow."
    );
    Ok(value as i32)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestoredBounds {
    /// None means center on the primary work area. Never reuse an offscreen origin.
    pub origin: Option<(i32, i32)>,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}
pub fn restore_bounds(
    saved: Option<WindowState>,
    monitors: &[Monitor],
    primary: &Monitor,
) -> Result<RestoredBounds> {
    ensure!(primary.valid(), "A valid primary monitor is required.");
    let Some(saved) = saved.filter(|saved| saved.valid()) else {
        return Ok(RestoredBounds {
            origin: None,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            maximized: false,
        });
    };
    let title_visible = |monitor: &Monitor| {
        if !monitor.valid() {
            return false;
        }
        let area = monitor.work_area;
        let width = (i64::from(saved.x) + i64::from(saved.width))
            .min(i64::from(area.x) + i64::from(area.width))
            - i64::from(saved.x.max(area.x));
        let height = (i64::from(saved.y) + 40).min(i64::from(area.y) + i64::from(area.height))
            - i64::from(saved.y.max(area.y));
        width >= i64::from(100.min(saved.width)) && height >= 1
    };
    // Include primary even when a platform display-list refresh omitted it.
    let visible = title_visible(primary) || monitors.iter().any(title_visible);
    Ok(if visible {
        RestoredBounds {
            origin: Some((saved.x, saved.y)),
            width: saved.width,
            height: saved.height,
            maximized: saved.maximized,
        }
    } else {
        RestoredBounds {
            origin: None,
            width: saved.width.min(primary.work_area.width).max(MIN_WIDTH),
            height: saved.height.min(primary.work_area.height).max(MIN_HEIGHT),
            maximized: saved.maximized,
        }
    })
}
#[derive(Clone, Copy, Debug)]
pub struct Capture {
    pub destroyed: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    pub current_pixels: Rect,
    pub normal_pixels: Rect,
}
pub fn capture_physical(capture: Capture, monitor: &Monitor) -> Result<Option<WindowState>> {
    if capture.destroyed || capture.minimized {
        return Ok(None);
    }
    let maximized = capture.maximized || capture.fullscreen;
    let bounds = monitor.to_logical(if maximized {
        capture.normal_pixels
    } else {
        capture.current_pixels
    })?;
    let state = WindowState {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
        maximized,
    };
    ensure!(
        state.valid(),
        "Window bounds do not match the persisted window-state schema."
    );
    Ok(Some(state))
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WindowStateKey {
    device: String,
    window: String,
}
impl WindowStateKey {
    /// Stable local window slots, not service UUIDs or remotely supplied paths.
    pub fn new(device: String, window: String) -> Result<Self> {
        let valid = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        };
        ensure!(
            valid(&device) && valid(&window),
            "Invalid device/window state key."
        );
        Ok(Self { device, window })
    }
    pub fn device(&self) -> &str {
        &self.device
    }
    pub fn window(&self) -> &str {
        &self.window
    }
}
/// Existing device-preferences adapter owns atomic persistence/JSON validation.
/// Read and write only the five Electron fields, cap stored window slots at 64.
pub trait WindowStateStore {
    fn read(&self, key: &WindowStateKey) -> Result<Option<WindowState>>;
    fn write(&self, key: &WindowStateKey, state: WindowState) -> Result<()>;
}
#[derive(Default)]
pub struct SaveDebounce {
    due: Option<Instant>,
}
impl SaveDebounce {
    pub fn changed(&mut self, now: Instant) {
        self.due = Some(now + SAVE_DELAY);
    }
    /// Capture current bounds at this point, not the bounds at the first move.
    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.due.is_some_and(|due| now >= due) {
            self.due = None;
            true
        } else {
            false
        }
    }
    /// Close/dispose captures immediately, even without a scheduled move.
    pub fn flush(&mut self) {
        self.due = None;
    }
}
