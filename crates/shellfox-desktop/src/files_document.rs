use super::control;
use crate::{root::Root, theme::Palette};
use gpui::{AnyElement, Context, div, prelude::*, px};
impl Root {
    pub(super) fn file_document(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(files) = &self.files else {
            return div().into_any_element();
        };
        let mut document = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::file_editor_key));
        if let Some(editor) = &files.editor {
            let dirty = editor.dirty(cx);
            let path = editor.file.path.clone();
            // Display the peer's path without asking this OS to interpret it.
            let name = path
                .rsplit(['/', '\\'])
                .find(|part| !part.is_empty())
                .unwrap_or(&path)
                .to_owned();
            document = document
                .child(
                    div()
                        .h(px(29.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(3.))
                        .px(px(4.))
                        .py(px(3.))
                        .border_b_1()
                        .border_color(palette.border)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(10.))
                                .text_color(palette.muted)
                                .child(format!("{name}{}", if dirty { " *" } else { "" })),
                        )
                        .child(
                            control("copy-file-path", "copy", palette).on_click(cx.listener(
                                move |root, _, _, cx| {
                                    if !root.require_files_source(cx) {
                                        return;
                                    }
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        path.clone(),
                                    ))
                                },
                            )),
                        )
                        .child(
                            control("save-file", "save", palette)
                                .when(!dirty || files.busy || editor.reload_required, |style| {
                                    style.opacity(0.4)
                                })
                                .on_click(cx.listener(|root, _, _, cx| root.save_file(cx))),
                        )
                        .when(files.operation.is_some(), |bar| {
                            bar.child(
                                control("cancel-file-operation", "cancel", palette).on_click(
                                    cx.listener(|root, _, _, cx| root.cancel_file_operation(cx)),
                                ),
                            )
                        })
                        .child(
                            control("reload-file", "reload", palette)
                                .on_click(cx.listener(|root, _, _, cx| root.reload_file(cx))),
                        )
                        .child(control("show-file-explorer", "folder", palette).on_click(
                            cx.listener(|root, _, _, cx| root.open_file_explorer(true, cx)),
                        )),
                )
                .child(
                    div()
                        .id("file-editor-scroll")
                        .track_scroll(&editor.text.read(cx).scroll)
                        .overflow_y_scroll()
                        .flex_1()
                        .min_h_0()
                        .child(editor.text.clone()),
                );
        } else {
            document = document.child(
                div()
                    .p(px(8.))
                    .text_color(palette.muted)
                    .child(if files.busy {
                        "Loading file…"
                    } else {
                        "Select a file."
                    })
                    .when(files.operation.is_some(), |line| {
                        line.child(
                            control("cancel-file-operation", "cancel", palette).on_click(
                                cx.listener(|root, _, _, cx| root.cancel_file_operation(cx)),
                            ),
                        )
                    }),
            );
        }
        document.into_any_element()
    }
}
