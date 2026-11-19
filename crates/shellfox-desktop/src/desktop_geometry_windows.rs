use crate::window_manager::state::{
    MIN_HEIGHT, MIN_WIDTH, Monitor, Rect, WindowState, restore_bounds,
};
use gpui::{App, Bounds, Window, WindowBounds, WindowOptions, point, px, size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::{
                GetWindowPlacement, GetWindowRect, IsIconic, WINDOW_EX_STYLE, WINDOWPLACEMENT,
                WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::BOOL,
};

#[derive(Clone)]
struct Screen {
    physical: Rect,
    work: Rect,
    logical: Rect,
    dpi: u32,
}
impl Screen {
    fn scale(&self) -> f64 {
        f64::from(self.dpi) / 96.
    }
    fn monitor(&self) -> Monitor {
        let scale = self.scale();
        let enclosing = scale_rect(self.work, 1. / scale);
        let physical_origin = (self.work.x, self.work.y);
        Monitor {
            id: format!("{}:{}", self.physical.x, self.physical.y),
            work_area: Rect {
                x: self.logical.x + ((self.work.x - self.physical.x) as f64 / scale).floor() as i32,
                y: self.logical.y + ((self.work.y - self.physical.y) as f64 / scale).floor() as i32,
                ..enclosing
            },
            physical_origin,
            scale_factor: scale,
        }
    }
}

fn rect(value: RECT) -> Rect {
    Rect {
        x: value.left,
        y: value.top,
        width: value.right - value.left,
        height: value.bottom - value.top,
    }
}
fn scale_rect(value: Rect, scale: f64) -> Rect {
    let x = (value.x as f64 * scale).floor() as i32;
    let y = (value.y as f64 * scale).floor() as i32;
    Rect {
        x,
        y,
        width: ((value.x + value.width) as f64 * scale).ceil() as i32 - x,
        height: ((value.y + value.height) as f64 * scale).ceil() as i32 - y,
    }
}

unsafe extern "system" fn monitor(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(handle, &mut info) }.as_bool() {
        return BOOL(1);
    }
    let (mut dpi, mut dpi_y) = (0, 0);
    if unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) }.is_err()
        || dpi == 0
    {
        return BOOL(1);
    }
    let physical = rect(info.rcMonitor);
    let screens = unsafe { &mut *(data.0 as *mut Vec<Screen>) };
    screens.push(Screen {
        physical,
        work: rect(info.rcWork),
        logical: scale_rect(physical, 96. / f64::from(dpi)),
        dpi,
    });
    BOOL(1)
}

fn screens() -> Vec<Screen> {
    let mut screens: Vec<Screen> = Vec::new();
    if !unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor),
            LPARAM((&mut screens as *mut Vec<Screen>) as isize),
        )
    }
    .as_bool()
    {
        return Vec::new();
    }
    let Some(primary) = screens
        .iter()
        .position(|item| item.physical.x == 0 && item.physical.y == 0)
    else {
        return Vec::new();
    };
    screens.swap(0, primary);
    // As in Chromium, build DIP positions outward from the primary display.
    // Independent origin scaling would overlap physically adjacent mixed-DPI monitors.
    let mut placed = vec![false; screens.len()];
    let mut parents = vec![0];
    placed[0] = true;
    while let Some(parent) = parents.pop() {
        let a = screens[parent].clone();
        for (index, child) in screens.iter_mut().enumerate() {
            if placed[index] {
                continue;
            }
            if let Some((x, y)) = adjacent_origin(&a, child) {
                child.logical.x = x;
                child.logical.y = y;
                placed[index] = true;
                parents.push(index);
            }
        }
    }
    screens
}

fn adjacent_origin(a: &Screen, b: &Screen) -> Option<(i32, i32)> {
    let (pa, pb) = (a.physical, b.physical);
    let a_right = pa.x + pa.width;
    let b_right = pb.x + pb.width;
    let a_bottom = pa.y + pa.height;
    let b_bottom = pb.y + pb.height;
    if pa.y.max(pb.y) <= a_bottom.min(b_bottom) {
        let y = a.logical.y + ((pb.y - pa.y) as f64 / a.scale()).round() as i32;
        if a_right == pb.x {
            return Some((a.logical.x + a.logical.width, y));
        }
        if b_right == pa.x {
            return Some((a.logical.x - b.logical.width, y));
        }
    }
    if pa.x.max(pb.x) <= a_right.min(b_right) {
        let x = a.logical.x + ((pb.x - pa.x) as f64 / a.scale()).round() as i32;
        if a_bottom == pb.y {
            return Some((x, a.logical.y + a.logical.height));
        }
        if b_bottom == pa.y {
            return Some((x, a.logical.y - b.logical.height));
        }
    }
    None
}

fn intersection(a: Rect, b: Rect) -> i64 {
    let width = ((a.x + a.width).min(b.x + b.width) - a.x.max(b.x)).max(0);
    let height = ((a.y + a.height).min(b.y + b.height) - a.y.max(b.y)).max(0);
    i64::from(width) * i64::from(height)
}

fn nonclient(dpi: u32) -> Option<(i32, i32)> {
    let mut frame = RECT::default();
    unsafe {
        AdjustWindowRectExForDpi(
            &mut frame,
            WS_OVERLAPPEDWINDOW,
            false,
            WINDOW_EX_STYLE(0),
            dpi,
        )
    }
    .ok()?;
    Some((frame.right - frame.left, frame.bottom - frame.top))
}

pub(super) fn configure(cx: &App, saved: Option<WindowState>, options: &mut WindowOptions) {
    let screens = screens();
    let Some(first) = screens.first() else {
        return;
    };
    let monitors: Vec<_> = screens.iter().map(Screen::monitor).collect();
    let Ok(restored) = restore_bounds(saved, &monitors, &first.monitor()) else {
        return;
    };
    let primary = first.monitor().work_area;
    let (x, y) = restored.origin.unwrap_or_else(|| {
        (
            primary.x + (primary.width - restored.width).max(0) / 2,
            primary.y + (primary.height - restored.height).max(0) / 2,
        )
    });
    let logical = Rect {
        x,
        y,
        width: restored.width,
        height: restored.height,
    };
    let Some(target) = screens
        .iter()
        .max_by_key(|screen| intersection(screen.logical, logical))
    else {
        return;
    };
    let Ok(physical) = target.monitor().to_physical(logical) else {
        return;
    };
    let Some((frame_w, frame_h)) = nonclient(target.dpi) else {
        return;
    };
    let scale = target.scale() as f32;
    // GPUI's Win32 initial placement consumes client dimensions in workspace
    // coordinates. Electron persists the outer dimensions in screen DIPs.
    let bounds = Bounds::new(
        point(
            px((physical.x + frame_w / 2 - (target.work.x - target.physical.x)) as f32 / scale),
            px((physical.y + frame_h / 2 - (target.work.y - target.physical.y)) as f32 / scale),
        ),
        size(
            px((physical.width - frame_w) as f32 / scale),
            px((physical.height - frame_h) as f32 / scale),
        ),
    );
    // Map GPUI's independently scaled display coordinates to the Win32 monitor.
    let expected = scale_rect(target.physical, 1. / target.scale());
    options.display_id = cx
        .displays()
        .into_iter()
        .find(|display| {
            let bounds = display.bounds();
            (f32::from(bounds.origin.x) - expected.x as f32).abs() <= 2.
                && (f32::from(bounds.origin.y) - expected.y as f32).abs() <= 2.
        })
        .map(|display| display.id());
    options.window_bounds = Some(if restored.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    });
    options.window_min_size = Some(size(
        px(MIN_WIDTH as f32 - frame_w as f32 / scale),
        px(MIN_HEIGHT as f32 - frame_h as f32 / scale),
    ));
}

pub(super) fn capture(window: &Window) -> Option<WindowState> {
    let native = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::Win32(native) = native.as_raw() else {
        return None;
    };
    let hwnd = HWND(native.hwnd.get() as *mut _);
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let maximized = window.is_maximized() || window.is_fullscreen();
    let mut current = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut current) }.ok()?;
    let current = rect(current);
    let screens = screens();
    let screen = screens
        .iter()
        .max_by_key(|screen| intersection(screen.physical, current))?;
    let physical = if maximized {
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        unsafe { GetWindowPlacement(hwnd, &mut placement) }.ok()?;
        let normal = rect(placement.rcNormalPosition);
        Rect {
            x: normal.x + screen.work.x - screen.physical.x,
            y: normal.y + screen.work.y - screen.physical.y,
            ..normal
        }
    } else {
        current
    };
    let scale = screen.scale();
    let dimensions = scale_rect(physical, 1. / scale);
    let state = WindowState {
        x: screen.logical.x + ((physical.x - screen.physical.x) as f64 / scale).round() as i32,
        y: screen.logical.y + ((physical.y - screen.physical.y) as f64 / scale).round() as i32,
        width: dimensions.width,
        height: dimensions.height,
        maximized,
    };
    state.valid().then_some(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_the_scaled_secondary_against_the_primary_dip_edge() {
        let primary = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let secondary = Rect {
            x: -2560,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let a = Screen {
            physical: primary,
            work: primary,
            logical: primary,
            dpi: 96,
        };
        let mut b = Screen {
            physical: secondary,
            work: secondary,
            logical: scale_rect(secondary, 2. / 3.),
            dpi: 144,
        };
        let (x, y) = adjacent_origin(&a, &b).unwrap();
        b.logical.x = x;
        b.logical.y = y;
        assert_eq!(b.logical.x + b.logical.width, 0);
        let restored = b
            .monitor()
            .to_physical(Rect {
                x,
                y,
                width: 1000,
                height: 700,
            })
            .unwrap();
        assert_eq!(
            (restored.x, restored.y, restored.width, restored.height),
            (-2560, 0, 1500, 1050)
        );
    }
}
