use crate::{root::Root, theme::Palette};
use gpui::{
    Context, CursorStyle, FocusHandle, MouseButton, MouseMoveEvent, Pixels, Window, div,
    prelude::*, px,
};

#[derive(Clone, Copy)]
pub(crate) enum ResizeAxis {
    Width,
    Height,
}

pub(crate) struct FileResize {
    pub axis: ResizeAxis,
    pub start: Pixels,
    pub value: f32,
}

impl ResizeAxis {
    fn default_size(self) -> f32 {
        match self {
            Self::Width => 420.,
            Self::Height => 220.,
        }
    }
    pub fn clamp(self, root: &Root, window: &Window, size: f32) -> f32 {
        let (min, max) = match self {
            Self::Width => (300_f32, (root.workspace_width(window) - 200.).max(0.)),
            Self::Height => (
                68_f32,
                (f32::from(window.viewport_size().height) - 228.).max(0.),
            ),
        };
        size.clamp(min.min(max), max)
    }
    fn value(self, root: &Root) -> f32 {
        root.files
            .panel
            .as_ref()
            .map_or(self.default_size(), |files| match self {
                Self::Width => files.width,
                Self::Height => files.browser_height,
            })
    }
    fn coordinate(self, point: gpui::Point<Pixels>) -> Pixels {
        match self {
            Self::Width => -point.x,
            Self::Height => point.y,
        }
    }
    fn set(self, root: &mut Root, window: &Window, size: f32) {
        let size = self.clamp(root, window, size);
        if let Some(files) = &mut root.files.panel {
            match self {
                Self::Width => files.width = size,
                Self::Height => files.browser_height = size,
            }
        }
    }
}

impl Root {
    pub(crate) fn resize_files(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = self
            .files
            .panel
            .as_ref()
            .and_then(|files| files.resize.as_ref())
        else {
            return false;
        };
        if event.pressed_button == Some(MouseButton::Left) {
            drag.axis.set(
                self,
                window,
                drag.value + f32::from(drag.axis.coordinate(event.position) - drag.start),
            );
        } else {
            stop(self, cx);
        }
        true
    }
}

fn stop(root: &mut Root, cx: &mut Context<Root>) {
    if let Some(files) = &mut root.files.panel
        && files.resize.take().is_some()
    {
        persist(files, cx);
    }
}

fn persist(files: &super::Files, cx: &gpui::App) {
    super::layout_prefs::FileLayout {
        width: files.width,
        browser_height: files.browser_height,
    }
    .save(cx);
}

pub(crate) fn files_resizer(
    axis: ResizeAxis,
    palette: Palette,
    focus: &FocusHandle,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let width = matches!(axis, ResizeAxis::Width);
    let focus = focus.clone();
    div()
        .id(if width {
            "file-width-resizer"
        } else {
            "file-browser-resizer"
        })
        .track_focus(&focus)
        .tab_stop(true)
        .flex_shrink_0()
        .bg(palette.sidebar)
        .when(width, |ent| {
            ent.absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(4.))
                .border_r_1()
                .cursor(CursorStyle::ResizeColumn)
        })
        .when(!width, |ent| {
            ent.w_full()
                .h(px(4.))
                .border_b_1()
                .cursor(CursorStyle::ResizeRow)
        })
        .border_color(palette.border)
        .hover(|style| style.bg(palette.accent))
        .on_key_down(
            cx.listener(move |root, event: &gpui::KeyDownEvent, window, cx| {
                let step = if event.keystroke.modifiers.shift {
                    32.
                } else {
                    4.
                };
                let value = axis.clamp(root, window, axis.value(root));
                let next = match (axis, event.keystroke.key.as_str()) {
                    (_, "home") => axis.default_size(),
                    (ResizeAxis::Width, "left") | (ResizeAxis::Height, "down") => value + step,
                    (ResizeAxis::Width, "right") | (ResizeAxis::Height, "up") => value - step,
                    _ => return,
                };
                axis.set(root, window, next);
                if let Some(files) = &root.files.panel {
                    persist(files, cx);
                }
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, event: &gpui::MouseDownEvent, window, cx| {
                window.focus(&focus);
                if event.click_count == 2 {
                    axis.set(root, window, axis.default_size());
                    if let Some(files) = &mut root.files.panel {
                        files.resize = None;
                        persist(files, cx);
                    }
                } else {
                    let value = axis.clamp(root, window, axis.value(root));
                    if let Some(files) = &mut root.files.panel {
                        files.resize = Some(FileResize {
                            axis,
                            start: axis.coordinate(event.position),
                            value,
                        });
                    }
                }
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|root, _, _, cx| {
                stop(root, cx);
                cx.notify();
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|root, _, _, cx| {
                stop(root, cx);
                cx.notify();
            }),
        )
}
