//! One native factory for bootstrap and launch-requested windows.
use crate::{
    accessibility::{HiddenWindowPresentation, NativeAccessibility, WindowPresentation},
    options::Options,
    root::Root,
    window_manager::InitialSelection,
};
use anyhow::Result;
use gpui::{App, Bounds, WindowBounds, WindowHandle, WindowOptions, prelude::*, px, size};

#[cfg(windows)]
mod backdrop;

pub(crate) const WINDOW_TITLE: &str = concat!("Shellfox v", env!("SHELLFOX_VERSION"));
pub(crate) const ATTACHED_WINDOW_TITLE: &str =
    concat!("Shellfox v", env!("SHELLFOX_VERSION"), " · host client");

pub(crate) fn caption(cx: &App) -> &'static str {
    if crate::app_host::adapter::get(cx).is_some_and(|host| host.is_external()) {
        ATTACHED_WINDOW_TITLE
    } else {
        WINDOW_TITLE
    }
}

pub(crate) fn bootstrap(options: Options, initial: InitialSelection, trace: bool, cx: &mut App) {
    #[cfg(target_os = "linux")]
    if gpui::guess_compositor() == "Wayland" && cx.displays().is_empty() {
        use std::time::{Duration, Instant};
        // GPUI receives wl_output events only after its launch callback returns.
        let executor = cx.background_executor().clone();
        cx.spawn(async move |cx| {
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline {
                match cx.update(|cx| !cx.displays().is_empty()) {
                    Ok(true) => break,
                    Ok(false) => {
                        executor.timer(Duration::from_millis(16)).await;
                    }
                    Err(_) => return,
                }
            }
            let _ = cx.update(|cx| open_bootstrap(options, initial, trace, cx));
        })
        .detach();
        return;
    }
    open_bootstrap(options, initial, trace, cx);
}

fn open_bootstrap(options: Options, initial: InitialSelection, trace: bool, cx: &mut App) {
    crate::trace_ui_init(trace, "desktop_windows::open begin");
    match open(options, initial, cx) {
        Ok(window) => {
            crate::trace_ui_init(trace, "desktop_windows::open returned Ok");
            crate::launch_runtime::set_bootstrap(window, cx);
        }
        Err(error) => {
            crate::trace_ui_init(trace, "desktop_windows::open returned Err");
            crate::app_startup::record_init_failure(error, cx);
            crate::app_exit::quit(cx);
        }
    }
}

pub(crate) fn open(
    options: Options,
    initial: InitialSelection,
    cx: &mut App,
) -> Result<WindowHandle<Root>> {
    let acceptance = options.ui_acceptance;
    let caption = caption(cx);
    let bounds = Bounds::centered(
        None,
        if acceptance {
            size(px(1200.), px(760.))
        } else {
            size(px(1220.), px(820.))
        },
        cx,
    );
    let (window_options, tracker) = crate::desktop_geometry::WindowHistory::restore(
        cx,
        WindowOptions {
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(caption.into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(850.), px(600.))),
            #[cfg(target_os = "linux")]
            window_decorations: std::env::var_os("WAYLAND_DISPLAY")
                .or_else(|| std::env::var_os("WAYLAND_SOCKET"))
                .map(|_| gpui::WindowDecorations::Client),
            focus: !acceptance,
            ..Default::default()
        },
    );
    let (window_options, presentation) = HiddenWindowPresentation::prepare(
        window_options,
        if acceptance {
            WindowPresentation::WithoutActivation
        } else {
            WindowPresentation::Activate
        },
    )?;
    cx.open_window(window_options, move |window, cx| {
        #[cfg(windows)]
        backdrop::apply(window);
        let provider = NativeAccessibility::attach(window);
        let root = cx.new(|cx| Root::open(options, initial, provider, window, cx));
        if let Some(tracker) = tracker {
            root.update(cx, |root, cx| root.track_window_bounds(tracker, window, cx));
        }
        let close = root.downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            close
                .update(cx, |root, cx| root.close(window, cx))
                .unwrap_or(false)
        });
        let shown = root.clone();
        window.defer(cx, move |window, cx| {
            if acceptance {
                if let Err(error) = shown.update(cx, |root, cx| {
                    root.queue_presentation(window, &presentation, cx)
                }) {
                    eprintln!("Native window queue admission failed: {error}");
                    crate::app_startup::record_init_failure(error.into(), cx);
                    window.remove_window();
                }
            } else {
                match shown.update(cx, |root, cx| {
                    root.present_hidden(window, &presentation, cx)
                }) {
                    Ok(()) => shown.update(cx, |root, cx| root.start_services(window, cx)),
                    Err(error) => {
                        eprintln!("Native window setup failed: {error}");
                        crate::app_startup::record_init_failure(error.into(), cx);
                        window.remove_window();
                    }
                }
            }
        });
        root
    })
}
