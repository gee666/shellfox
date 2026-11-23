use super::super::Field;
use gpui::{Context, Div, Window, div, prelude::*, px};

pub(super) fn container(
    field: &Field,
    content_width: Option<f32>,
    height: f32,
    window: &Window,
    cx: &Context<Field>,
) -> Div {
    let palette = field.palette;
    let directory = field.directory;
    let multiline = field.multiline;
    let code = field.code_editor;
    let compact = field.compact;
    let session_title = field.session_title;
    let settings_input = field.settings_input;
    let focused = field.focus.is_focused(window);
    div()
        .relative()
        .w_full()
        .min_w_0()
        .when_some(content_width, |element, width| {
            element.w(px(width)).flex_shrink_0()
        })
        .when(!directory, |style| {
            style.px_2().bg(if multiline && code {
                palette.panel
            } else {
                palette.input_bg
            })
        })
        .when(!multiline && !directory, |style| {
            style
                .h(px(if session_title {
                    16.
                } else if compact {
                    field.line_height + 8.
                } else {
                    28.
                }))
                .when(compact, |style| style.px(px(4.)))
                .when(session_title, |style| style.px(px(2.)))
                .when(settings_input && !compact, |style| style.px(px(6.)))
                .border_1()
                .border_color(if (session_title || compact) && focused {
                    palette.accent
                } else {
                    palette.input_border
                })
                .rounded_sm()
        })
        .when(directory, |style| style.h(px(28.)))
        .when(multiline, |style| style.h(px(height)))
        .track_focus(&if settings_input || field.asset_editor || directory {
            field.focus.clone().tab_stop(true)
        } else {
            field.focus.clone()
        })
        .on_key_down(cx.listener(Field::key))
        .on_mouse_down(gpui::MouseButton::Left, cx.listener(Field::click))
        .on_mouse_move(cx.listener(Field::drag))
        .on_mouse_up(gpui::MouseButton::Left, cx.listener(Field::release))
        .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Field::release))
}
