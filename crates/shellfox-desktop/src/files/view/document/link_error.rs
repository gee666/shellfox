use super::super::{Control, control, text_button};
use crate::{accessibility::Role, files::Files, root::Root, theme::Palette};
use gpui::{AnyElement, Context, div, prelude::*, px};

const MESSAGE: &str = "File not found.";

/// A terminal link that named nothing: the message, the searched path with a copy
/// button, and Retry. Centred when no document is open, else a strip above it.
pub(super) fn render(
    root: &Root,
    files: &Files,
    centred: bool,
    palette: Palette,
    cx: &mut Context<Root>,
) -> Option<AnyElement> {
    let link = files.link_error.as_ref()?;
    Some(
        div()
            .p(px(8.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .when(centred, |view| view.flex_1().min_h_0().justify_center())
            .when(!centred, |view| {
                view.flex_shrink_0()
                    .border_b_1()
                    .border_color(palette.border)
            })
            .child(
                root.semantic_region(
                    "files/link-error",
                    Role::Label,
                    MESSAGE,
                    div()
                        .text_color(palette.status_color(0xf87171))
                        .child(MESSAGE),
                ),
            )
            .child(
                div()
                    .max_w_full()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .child(
                        div()
                            .min_w_0()
                            .font_family("Cascadia Code")
                            .text_size(px(11.))
                            .child(link.path.clone()),
                    )
                    .child(
                        Control::CopySearchedPath.wrap(
                            root,
                            "Copy searched path",
                            true,
                            None,
                            control("copy-searched-path", "copy", palette)
                                .tooltip(crate::assets::tooltip("Copy searched path", palette))
                                .on_click(
                                    cx.listener(|root, _, _, cx| root.copy_link_error_path(cx)),
                                ),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        text_button("retry-file-link", "Retry file", palette)
                            .on_click(cx.listener(|root, _, _, cx| root.retry_file_link(cx))),
                    )
                    .when(!centred, |actions| {
                        actions.child(
                            text_button("dismiss-link-error", "×", palette)
                                .w(px(24.))
                                .tooltip(crate::assets::tooltip("Dismiss", palette))
                                .on_click(cx.listener(|root, _, _, cx| {
                                    if let Some(files) = &mut root.files.panel {
                                        files.link_error = None;
                                        cx.notify();
                                    }
                                })),
                        )
                    }),
            )
            .into_any_element(),
    )
}
