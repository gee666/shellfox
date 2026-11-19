use crate::{root::Root, theme::Palette};
use gpui::{
    AnyElement, Context, MouseButton, MouseMoveEvent, PathBuilder, Window, canvas, div, point,
    prelude::*, px,
};

pub(crate) struct EnvironmentResize {
    height: f32,
    drag: Option<(f32, f32)>,
}
impl Default for EnvironmentResize {
    fn default() -> Self {
        Self {
            height: 180.,
            drag: None,
        }
    }
}
impl EnvironmentResize {
    pub(crate) fn height(&self, window: &Window) -> f32 {
        self.height.clamp(
            100.,
            (f32::from(window.viewport_size().height) * 0.36).max(100.),
        )
    }
}

impl Root {
    pub(crate) fn session_environment_handle(
        &self,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id("session-environment-resize")
            .absolute()
            .right_0()
            .bottom_0()
            .size(px(9.))
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let x = bounds.right() - px(1.5);
                        let y = bounds.bottom() - px(1.5);
                        let mut grip = PathBuilder::stroke(px(0.75));
                        for length in [3., 6.] {
                            grip.move_to(point(x - px(length), y));
                            grip.line_to(point(x, y - px(length)));
                        }
                        if let Ok(path) = grip.build() {
                            window.paint_path(path, palette.muted.opacity(0.8));
                        }
                    },
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|root, event: &gpui::MouseDownEvent, window, cx| {
                    let state = &mut root.sidebar_ui.environment_resize;
                    state.drag = Some((f32::from(event.position.y), state.height(window)));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|root, _, _, _| root.sidebar_ui.environment_resize.drag = None),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|root, _, _, _| root.sidebar_ui.environment_resize.drag = None),
            )
            .into_any_element()
    }

    pub(crate) fn resize_session_environment(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let state = &mut self.sidebar_ui.environment_resize;
        let Some((y, height)) = state.drag else {
            return false;
        };
        if event.pressed_button != Some(MouseButton::Left) || self.dialog.is_none() {
            state.drag = None;
            return false;
        }
        state.height = height + f32::from(event.position.y) - y;
        state.height = state.height(window);
        cx.notify();
        cx.stop_propagation();
        true
    }
}
