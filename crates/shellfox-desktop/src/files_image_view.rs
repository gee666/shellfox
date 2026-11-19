use crate::{
    files::image_editor::{Crop, Draft, ImageEditor, Transform},
    root::Root,
    theme::Palette,
};
use gpui::{
    AnyElement, Bounds, Context, MouseButton, Pixels, Point, Window, canvas, div, fill, img, point,
    prelude::*, px, rgb, size,
};

pub(super) fn render(
    editor: &ImageEditor,
    width: f32,
    height: f32,
    palette: Palette,
    busy: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let ready = match editor {
        ImageEditor::Ready(draft) => Some(draft),
        _ => None,
    };
    let locked = busy || ready.is_none_or(|draft| draft.working);
    let crop = ready.and_then(|draft| draft.crop);
    let dirty = ready.is_some_and(|draft| draft.edits > 0);
    let status = match editor {
        ImageEditor::Loading => "Loading image…".into(),
        ImageEditor::Failed(error) => error.clone(),
        ImageEditor::Ready(edit) => edit.error.clone().unwrap_or_else(|| {
            let (w, h) = edit.dimensions();
            format!("{w} × {h}{}", if dirty { " · Unsaved" } else { "" })
        }),
    };
    let mut toolbar = div()
        .id("image-editor-toolbar")
        .overflow_x_scroll()
        .flex()
        .gap(px(3.))
        .items_center()
        .mb(px(4.))
        .flex_shrink_0();
    for (name, hint, transform) in [
        ("rotate-left", "Rotate left", Transform::RotateLeft),
        ("rotate-right", "Rotate right", Transform::RotateRight),
        (
            "flip-horizontal",
            "Flip horizontal",
            Transform::FlipHorizontal,
        ),
        ("flip-vertical", "Flip vertical", Transform::FlipVertical),
    ] {
        toolbar = toolbar.child(
            action(name, hint, locked, false, palette)
                .on_click(cx.listener(move |root, _, _, cx| root.transform_image(transform, cx))),
        );
    }
    toolbar = toolbar
        .child(
            action("crop", "Crop", locked || crop.is_none(), false, palette).on_click(cx.listener(
                move |root, _, _, cx| {
                    if let Some(crop) = root.image_draft().and_then(|draft| draft.crop) {
                        root.transform_image(Transform::Crop(crop), cx);
                    }
                },
            )),
        )
        .child(
            action(
                "reset",
                "Reset",
                locked || (!dirty && crop.is_none()),
                false,
                palette,
            )
            .on_click(cx.listener(|root, _, _, cx| root.reset_image(cx))),
        )
        .child(
            action(
                "save",
                if busy { "Saving…" } else { "Save image" },
                locked || !dirty || ready.is_some_and(|draft| draft.reload_required),
                true,
                palette,
            )
            .on_click(cx.listener(|root, _, _, cx| root.save_edited_image(cx))),
        );
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .p(px(4.))
        .bg(rgb(0x17171b))
        .text_color(rgb(0xdddddd))
        .text_size(px(12.))
        .line_height(px(16.))
        .child(toolbar)
        .child(div().mb(px(4.)).child(status))
        .when_some(ready, |view, draft| {
            view.child(pixels(draft, width, height, cx))
        })
        .into_any_element()
}

fn action(
    icon: &'static str,
    hint: &'static str,
    disabled: bool,
    save: bool,
    palette: Palette,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(gpui::SharedString::from(format!("image-action-{icon}")))
        .w(px(26.))
        .h(px(26.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(0x242429))
        .rounded(px(3.))
        .border_1()
        .border_color(rgb(0x555555))
        .when(save && !disabled, |button| {
            button.border_color(rgb(0x548acb)).text_color(rgb(0xb9d8ff))
        })
        .when(disabled, |button| button.opacity(0.4))
        .when(!disabled, |button| {
            button
                .cursor_pointer()
                .tab_index(0)
                .hover(|style| style.bg(rgb(0x303037)))
        })
        .child(
            crate::assets::icon(match icon {
                "rotate-left" => "image-rotate-left",
                "rotate-right" => "image-rotate-right",
                "flip-horizontal" => "image-flip-horizontal",
                "flip-vertical" => "image-flip-vertical",
                "crop" => "image-crop",
                "reset" => "image-reset",
                _ => "image-save",
            })
            .size(px(16.)),
        )
        .tooltip(crate::assets::tooltip(hint, palette))
}

fn pixels(draft: &Draft, width: f32, height: f32, cx: &mut Context<Root>) -> impl IntoElement {
    let (w, h) = draft.dimensions();
    let scale = ((width - 8.) / w as f32)
        .min((height * 0.65) / h as f32)
        .clamp(0.001, 1.);
    let (display_w, display_h) = (w as f32 * scale, h as f32 * scale);
    let bounds_slot = draft.bounds.clone();
    div()
        .id("image-editor-viewport")
        .overflow_y_scroll()
        .flex_1()
        .min_h_0()
        .child(
            div().w_full().flex().justify_center().child(
                div()
                    .id("image-editor-canvas")
                    .relative()
                    .w(px(display_w))
                    .h(px(display_h))
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                bounds_slot.set(bounds);
                                checkerboard(bounds, window);
                            },
                        )
                        .size_full(),
                    )
                    .child(img(draft.frame.clone()).absolute().inset_0().size_full())
                    .when_some(draft.crop, |view, crop| {
                        view.child(
                            div()
                                .absolute()
                                .left(px(crop.x as f32 * scale))
                                .top(px(crop.y as f32 * scale))
                                .w(px(crop.width as f32 * scale))
                                .h(px(crop.height as f32 * scale))
                                .border_2()
                                .border_dashed()
                                .border_color(rgb(0x1478df))
                                .bg(gpui::rgba(0x1478df22)),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|root, event: &gpui::MouseDownEvent, _, cx| {
                            if let Some(edit) = root.image_draft() {
                                edit.anchor = Some(image_point(edit, event.position));
                                edit.crop = None;
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|root, event: &gpui::MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            update_crop(root, event.position, false, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|root, event: &gpui::MouseUpEvent, _, cx| {
                            update_crop(root, event.position, true, cx)
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|root, event: &gpui::MouseUpEvent, _, cx| {
                            update_crop(root, event.position, true, cx)
                        }),
                    ),
            ),
        )
}

fn image_point(draft: &Draft, cursor: Point<Pixels>) -> (u32, u32) {
    let bounds = draft.bounds.get();
    let (w, h) = draft.dimensions();
    let project = |cursor: Pixels, origin: Pixels, size: Pixels, count: u32| {
        (f32::from(cursor - origin) / f32::from(size).max(1.) * count as f32)
            .round()
            .clamp(0., count as f32) as u32
    };
    (
        project(cursor.x, bounds.origin.x, bounds.size.width, w),
        project(cursor.y, bounds.origin.y, bounds.size.height, h),
    )
}

fn update_crop(root: &mut Root, point: Point<Pixels>, done: bool, cx: &mut Context<Root>) {
    let Some(edit) = root.image_draft() else {
        return;
    };
    let Some((start_x, start_y)) = edit.anchor else {
        return;
    };
    let (end_x, end_y) = image_point(edit, point);
    let crop = Crop {
        x: start_x.min(end_x),
        y: start_y.min(end_y),
        width: start_x.abs_diff(end_x),
        height: start_y.abs_diff(end_y),
    };
    edit.crop = (crop.width > 0 && crop.height > 0).then_some(crop);
    if done {
        edit.anchor = None;
    }
    cx.notify();
}

fn checkerboard(bounds: Bounds<Pixels>, window: &mut Window) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    for y in (0..h.ceil() as u32).step_by(8) {
        for x in (0..w.ceil() as u32).step_by(8) {
            let (left, top) = (x as f32, y as f32);
            let color = if ((x + y) / 8) % 2 == 0 {
                0xdddddd
            } else {
                0xffffff
            };
            window.paint_quad(fill(
                Bounds::new(
                    bounds.origin + point(px(left), px(top)),
                    size(px((w - left).min(8.)), px((h - top).min(8.))),
                ),
                rgb(color),
            ));
        }
    }
}
