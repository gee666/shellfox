//! The taskbar-free work area of a GPUI display. GPUI 0.2 only exposes the full monitor.
use crate::window_manager::state::Screen;
use gpui::{Bounds, PlatformDisplay, Point, point, px, size};
use windows::{
    Win32::{
        Foundation::{LPARAM, RECT},
        Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
        UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
    },
    core::BOOL,
};

struct Monitor {
    full: RECT,
    work: RECT,
    scale: f32,
}

unsafe extern "system" fn collect(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (mut dpi, mut dpi_y) = (0, 0);
    // SAFETY: Win32 fills the stack structs; `data` is the Vec passed by `monitors`.
    unsafe {
        if GetMonitorInfoW(handle, &mut info).as_bool()
            && GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y).is_ok()
            && dpi > 0
        {
            (*(data.0 as *mut Vec<Monitor>)).push(Monitor {
                full: info.rcMonitor,
                work: info.rcWork,
                scale: dpi as f32 / 96.,
            });
        }
    }
    BOOL(1)
}

fn monitors() -> Vec<Monitor> {
    let mut monitors = Vec::new();
    // SAFETY: the callback only runs during this call and writes to `monitors`.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut monitors as *mut Vec<Monitor> as isize),
        );
    }
    monitors
}

pub(super) fn screen(display: &dyn PlatformDisplay) -> Screen {
    let bounds = display.bounds();
    let mut screen = Screen::full(display.uuid().ok().map(|uuid| uuid.to_string()), bounds);
    // GPUI scales each monitor's rectangle by that monitor's DPI.
    let near = |a: f32, b: i32, scale: f32| (a - b as f32 / scale).abs() <= 1.;
    let Some(monitor) = monitors().into_iter().find(|monitor| {
        let (full, scale) = (monitor.full, monitor.scale);
        near(f32::from(bounds.origin.x), full.left, scale)
            && near(f32::from(bounds.origin.y), full.top, scale)
            && near(f32::from(bounds.size.width), full.right - full.left, scale)
            && near(f32::from(bounds.size.height), full.bottom - full.top, scale)
    }) else {
        return screen;
    };
    let (full, work, scale) = (monitor.full, monitor.work, monitor.scale);
    let logical = |value: i32| px(value as f32 / scale);
    let offset: Point<_> = point(logical(work.left - full.left), logical(work.top - full.top));
    screen.work = Bounds::new(
        offset,
        size(logical(work.right - work.left), logical(work.bottom - work.top)),
    );
    screen.workspace = offset;
    screen
}
