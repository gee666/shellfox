use crate::{
    host_client::HostClient,
    root::Root,
    window_manager::state::{SAVE_DELAY, WindowState},
};
use gpui::{App, Context, Global, Task, Window, WindowOptions};
use shellfox_service::native::{NativeAction, NativeResponse};

#[cfg(windows)]
#[path = "desktop_geometry_windows.rs"]
mod platform;
#[cfg(not(windows))]
#[path = "desktop_geometry_unix.rs"]
mod platform;

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
            saved: saved.filter(|state| state.valid()),
            claimed: false,
        })
    }

    pub(crate) fn restore(
        cx: &mut App,
        mut options: WindowOptions,
    ) -> (WindowOptions, Option<WindowTracker>) {
        let Some(history) = cx.try_global::<Self>() else {
            return (options, None);
        };
        if history.claimed {
            return (options, None);
        }
        let saved = history.saved;
        let client = history.client.clone();
        cx.global_mut::<Self>().claimed = true;
        platform::configure(cx, saved, &mut options);
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
                    .update_in(cx, |root, window, _| {
                        root.capture_window_bounds(window, false)
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

    pub(crate) fn flush_window_bounds(&mut self, window: &Window) -> Option<WindowWrite> {
        if let Some(tracker) = &mut self.window_tracker {
            tracker.debounce = None;
        }
        self.capture_window_bounds(window, true)
    }

    fn capture_window_bounds(&mut self, window: &Window, force: bool) -> Option<WindowWrite> {
        let tracker = self.window_tracker.as_mut()?;
        let state = platform::capture(window)?;
        if !force && tracker.last == Some(state) {
            return None;
        }
        tracker.last = Some(state);
        Some(WindowWrite(tracker.client.clone(), state))
    }
}
