use crate::{
    host_client::HostClient,
    root::Root,
    window_manager::state::{SAVE_DELAY, Screen, WindowState, min_size, place, record},
};
use gpui::{App, Bounds, Context, Global, Task, Window, WindowBounds, WindowOptions};
use shellfox_service::native::{NativeAction, NativeResponse};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win32;

pub(crate) struct WindowHistory {
    client: HostClient,
    saved: Option<WindowState>,
    claimed: bool,
}
impl Global for WindowHistory {}

impl WindowHistory {
    pub(crate) async fn read(client: HostClient) -> anyhow::Result<Self> {
        let NativeResponse::WindowState(saved) =
            client.execute(NativeAction::ReadWindowState).await?
        else {
            anyhow::bail!("Invalid desktop window-state response");
        };
        Ok(Self {
            client,
            saved: saved.filter(WindowState::valid),
            claimed: false,
        })
    }

    pub(crate) fn restore(
        cx: &mut App,
        mut options: WindowOptions,
    ) -> (WindowOptions, Option<WindowTracker>) {
        let Some(history) = cx.try_global::<Self>().filter(|history| !history.claimed) else {
            if wayland() {
                configure(cx, None, &mut options);
            }
            return (options, None);
        };
        let saved = history.saved.clone();
        let client = history.client.clone();
        cx.global_mut::<Self>().claimed = true;
        configure(cx, saved.as_ref(), &mut options);
        let tracker = WindowTracker {
            client,
            last: saved,
            debounce: None,
        };
        (options, Some(tracker))
    }
}

pub(crate) struct WindowTracker {
    client: HostClient,
    last: Option<WindowState>,
    debounce: Option<Task<()>>,
}

pub(crate) struct WindowWrite(HostClient, WindowState);
impl WindowWrite {
    pub(crate) async fn finish(self) {
        if let Err(error) = self.0.execute(NativeAction::SaveWindowState(self.1)).await {
            eprintln!("Could not save desktop window bounds: {error:#}");
        }
    }
}

impl Root {
    pub(crate) fn track_window_bounds(
        &mut self,
        tracker: WindowTracker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_tracker = Some(tracker);
        cx.observe_window_bounds(window, |root, window, cx| {
            if root.close.closing {
                return;
            }
            let Some(tracker) = &mut root.window_tracker else {
                return;
            };
            tracker.debounce = Some(cx.spawn_in(window, async move |root, cx| {
                cx.background_executor().timer(SAVE_DELAY).await;
                let write = root
                    .update_in(cx, |root, window, cx| {
                        root.capture_window_bounds(window, false, cx)
                    })
                    .ok()
                    .flatten();
                if let Some(write) = write {
                    write.finish().await;
                }
            }));
        })
        .detach();
    }

    pub(crate) fn flush_window_bounds(&mut self, window: &Window, cx: &App) -> Option<WindowWrite> {
        if let Some(tracker) = &mut self.window_tracker {
            tracker.debounce = None;
        }
        self.capture_window_bounds(window, true, cx)
    }

    fn capture_window_bounds(
        &mut self,
        window: &Window,
        force: bool,
        cx: &App,
    ) -> Option<WindowWrite> {
        let tracker = self.window_tracker.as_mut()?;
        let state = capture(window, cx)?;
        if !force && tracker.last.as_ref() == Some(&state) {
            return None;
        }
        tracker.last = Some(state.clone());
        Some(WindowWrite(tracker.client.clone(), state))
    }
}

#[cfg(target_os = "linux")]
fn wayland() -> bool {
    gpui::guess_compositor() == "Wayland"
}

#[cfg(not(target_os = "linux"))]
fn wayland() -> bool {
    false
}

#[cfg(target_os = "macos")]
use macos::screen;
#[cfg(windows)]
use win32::screen;

#[cfg(not(any(windows, target_os = "macos")))]
fn screen(display: &dyn gpui::PlatformDisplay) -> Screen {
    Screen::full(
        display.uuid().ok().map(|uuid| uuid.to_string()),
        display.bounds(),
    )
}

fn configure(cx: &App, saved: Option<&WindowState>, options: &mut WindowOptions) {
    let displays = cx.displays();
    let mut screens: Vec<_> = displays
        .iter()
        .map(|display| screen(display.as_ref()))
        .collect();
    let primary = cx.primary_display().and_then(|primary| {
        displays
            .iter()
            .position(|display| display.id() == primary.id())
    });
    let primary = if wayland() {
        // The compositor picks the output and the position, so fit every output.
        let Some(first) = screens.first() else {
            return;
        };
        let mut fit = first.bounds.size;
        for screen in &screens {
            fit = fit.min(&screen.bounds.size);
        }
        screens = vec![Screen::full(None, Bounds::new(first.bounds.origin, fit))];
        0
    } else {
        primary.unwrap_or(0)
    };
    let saved = saved.filter(|_| !wayland());
    let Some(placement) = place(saved, &screens, primary) else {
        eprintln!("Window restore: no displays are available yet; using the default placement");
        return;
    };
    if let Some(id) = saved.and_then(|saved| saved.display.as_ref())
        && screens[placement.screen].uuid.as_ref() != Some(id)
    {
        eprintln!("Window restore: the saved display is gone; using the primary display");
    }
    options.display_id = (!wayland())
        .then(|| displays.get(placement.screen).map(|display| display.id()))
        .flatten();
    options.window_bounds = Some(if placement.maximized {
        WindowBounds::Maximized(placement.bounds)
    } else {
        WindowBounds::Windowed(placement.bounds)
    });
    options.window_min_size = Some(min_size());
    // AccessKit's native adapter is per OS window. Native macOS tab grouping
    // breaks that mapping; application tabs remain ordinary Shellfox tabs.
    options.tabbing_identifier = None;
}

fn capture(window: &Window, cx: &App) -> Option<WindowState> {
    let (normal, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, true),
    };
    if wayland() {
        // Wayland has no global window position; only the size is restored.
        return record(
            gpui::Bounds {
                origin: gpui::Point::default(),
                ..normal
            },
            maximized,
            None,
        );
    }
    let display = window.display(cx).map(|display| screen(display.as_ref()));
    record(normal, maximized, display.as_ref())
}

/// AppKit's visible frame relative to its screen frame, flipped to GPUI's downward y axis.
/// Both rects are `[x, y, width, height]` in AppKit points, y up from the primary display.
#[cfg(any(target_os = "macos", test))]
fn appkit_work_area(frame: [f64; 4], visible: [f64; 4]) -> Option<Bounds<gpui::Pixels>> {
    let [fx, fy, fw, fh] = frame;
    let [vx, vy, vw, vh] = visible;
    let (left, top) = (vx - fx, (fy + fh) - (vy + vh));
    // Half a point of slack for fractional scaling.
    let inside = left >= -0.5
        && top >= -0.5
        && vw > 0.
        && vh > 0.
        && left + vw <= fw + 0.5
        && top + vh <= fh + 0.5;
    inside.then(|| {
        Bounds::new(
            gpui::point(gpui::px(left.max(0.) as f32), gpui::px(top.max(0.) as f32)),
            gpui::size(gpui::px(vw as f32), gpui::px(vh as f32)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::appkit_work_area;
    use gpui::{Bounds, point, px, size};

    #[test]
    fn the_menu_bar_and_a_bottom_dock_leave_the_middle() {
        // A 1440x900 display with a 25 pt menu bar and a 70 pt Dock at the bottom.
        let work = appkit_work_area([0., 0., 1440., 900.], [0., 70., 1440., 805.]);
        assert_eq!(
            work,
            Some(Bounds::new(
                point(px(0.), px(25.)),
                size(px(1440.), px(805.))
            ))
        );
    }

    #[test]
    fn a_secondary_display_above_the_primary_with_a_left_dock() {
        let work = appkit_work_area([-200., 900., 1920., 1080.], [-120., 900., 1840., 1055.]);
        assert_eq!(
            work,
            Some(Bounds::new(
                point(px(80.), px(25.)),
                size(px(1840.), px(1055.))
            ))
        );
    }

    #[test]
    fn a_visible_frame_outside_its_screen_is_ignored() {
        assert_eq!(
            appkit_work_area([0., 0., 1440., 900.], [0., 0., 1600., 900.]),
            None
        );
    }
}
