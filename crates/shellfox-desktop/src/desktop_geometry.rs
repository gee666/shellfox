use crate::{
    host_client::HostClient,
    root::Root,
    window_manager::state::{SAVE_DELAY, Screen, WindowState, min_size, place, record},
};
use gpui::{App, Context, Global, PlatformDisplay, Task, Window, WindowBounds, WindowOptions};
use shellfox_service::native::{NativeAction, NativeResponse};

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
            if root.closing_window {
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

fn wayland() -> bool {
    #[cfg(target_os = "linux")]
    return gpui::guess_compositor() == "Wayland";
    #[cfg(not(target_os = "linux"))]
    false
}

fn screen(display: &dyn PlatformDisplay) -> Screen {
    Screen {
        uuid: display.uuid().ok().map(|uuid| uuid.to_string()),
        bounds: display.bounds(),
    }
}

fn configure(cx: &App, saved: Option<&WindowState>, options: &mut WindowOptions) {
    let displays = cx.displays();
    let mut screens: Vec<_> = displays.iter().map(|display| screen(display.as_ref())).collect();
    let primary = cx.primary_display().and_then(|primary| {
        displays.iter().position(|display| display.id() == primary.id())
    });
    let primary = if wayland() {
        // The compositor picks the output and the position, so fit every output.
        let Some(mut fit) = screens.first().cloned() else {
            return;
        };
        for screen in &screens {
            fit.bounds.size = fit.bounds.size.min(&screen.bounds.size);
        }
        fit.uuid = None;
        screens = vec![fit];
        0
    } else {
        primary.unwrap_or(0)
    };
    let saved = saved.filter(|_| !wayland());
    let Some(placement) = place(saved, &screens, primary) else {
        return;
    };
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
        return record(gpui::Bounds { origin: gpui::Point::default(), ..normal }, maximized, None);
    }
    let display = window.display(cx).map(|display| screen(display.as_ref()));
    record(normal, maximized, display.as_ref())
}
