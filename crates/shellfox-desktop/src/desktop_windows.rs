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
#[path = "window_backdrop.rs"]
mod backdrop;

pub(crate) const WINDOW_TITLE: &str = concat!("Shellfox v", env!("SHELLFOX_VERSION"));
pub(crate) const ATTACHED_WINDOW_TITLE: &str = concat!("Shellfox v", env!("SHELLFOX_VERSION"), " · host client");

pub(crate) fn caption(cx: &App) -> &'static str {
    if crate::app_host::adapter::get(cx).is_some_and(|host| host.is_attached()) { ATTACHED_WINDOW_TITLE } else { WINDOW_TITLE }
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
    let (window_options, tracker) = crate::desktop_geometry::WindowHistory::restore(cx,
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
                match shown.update(cx, |root, cx| root.present_hidden(window, &presentation, cx)) {
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
