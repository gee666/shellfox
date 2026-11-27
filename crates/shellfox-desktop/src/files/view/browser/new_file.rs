use super::super::{Control, icon_button, text_button, text_button_enabled};
use crate::{files::Files, root::Root, theme::Palette};
use gpui::{AnyElement, Context, div, prelude::*, px};

/// The "+" at the end of the path line; it opens and closes the name form.
pub(super) fn button(
    root: &Root,
    files: &Files,
    palette: Palette,
    cx: &mut Context<Root>,
) -> AnyElement {
    let open = files.new_file.is_some();
    let enabled = !files.busy() && (open || files.listing.is_some());
    Control::NewFile.wrap(
        root,
        "New file",
        enabled,
        None,
        icon_button("new-file", "plus", palette, enabled)
            .when(open, |button| {
                button.bg(palette.hover).text_color(palette.accent)
            })
            .tooltip(crate::assets::tooltip("New file", palette))
            .when(enabled, |button| {
                button.on_click(cx.listener(|root, _, window, cx| root.toggle_new_file(window, cx)))
            }),
    )
}

/// One name field with Create and ×, under the path line.
pub(super) fn form(files: &Files, palette: Palette, cx: &mut Context<Root>) -> Option<AnyElement> {
    let form = files.new_file.as_ref()?;
    // Root does not observe the field, so Create stays enabled; an empty name gets its error.
    let busy = files.busy();
    Some(
        div()
            .flex_shrink_0()
            .px(px(4.))
            .py(px(3.))
            .flex()
            .flex_col()
            .gap(px(3.))
            .border_b_1()
            .border_color(palette.border)
            .on_key_down(cx.listener(Root::new_file_key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .child(div().flex_1().min_w_0().child(form.name.clone()))
                    .child(
                        text_button_enabled("create-new-file", "Create", palette, !busy).when(
                            !busy,
                            |button| {
                                button.on_click(
                                    cx.listener(|root, _, _, cx| root.create_new_file(cx)),
                                )
                            },
                        ),
                    )
                    .child(
                        text_button("cancel-new-file", "×", palette)
                            .w(px(24.))
                            .tooltip(crate::assets::tooltip("Cancel", palette))
                            .on_click(
                                cx.listener(|root, _, window, cx| root.cancel_new_file(window, cx)),
                            ),
                    ),
            )
            .when_some(form.error.clone(), |form, error| {
                form.child(
                    div()
                        .text_size(px(11.))
                        .text_color(palette.status_color(0xf87171))
                        .child(error),
                )
            })
            .into_any_element(),
    )
}
