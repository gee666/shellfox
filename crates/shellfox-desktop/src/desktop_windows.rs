//! One native factory for bootstrap and launch-requested windows.
use crate::{
    accessibility::{HiddenWindowPresentation, NativeAccessibility, WindowPresentation},
    options::Options,
    root::Root,
    window_manager::InitialSelection,
};
use anyhow::Result;
use gpui::{App, Bounds, WindowBounds, WindowHandle, WindowOptions, prelude::*, px, size};

pub(crate) fn open(
    options: Options,
    initial: InitialSelection,
    cx: &mut App,
) -> Result<WindowHandle<Root>> {
    let acceptance = options.ui_acceptance;
    let bounds = Bounds::centered(
        None,
        if acceptance {
            size(px(1200.), px(760.))
        } else {
            size(px(1220.), px(820.))
        },
        cx,
    );
    let (window_options, presentation) = HiddenWindowPresentation::prepare(
        WindowOptions {
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(
                    if acceptance {
                        format!("Shellfox v{}", env!("SHELLFOX_PRODUCT_VERSION"))
                    } else {
                        "Shellfox · Rust workspace".to_owned()
                    }
                    .into(),
                ),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(850.), px(600.))),
            focus: !acceptance,
            ..Default::default()
        },
        if acceptance {
            WindowPresentation::WithoutActivation
        } else {
            WindowPresentation::Activate
        },
    )?;
    cx.open_window(window_options, move |window, cx| {
        let provider = NativeAccessibility::attach(window);
        let root = cx.new(|cx| Root::open(options, initial, provider, window, cx));
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
                match shown.update(cx, |root, _| root.present_hidden(window, &presentation)) {
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
