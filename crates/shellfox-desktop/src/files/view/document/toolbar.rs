//! The open file's toolbar: path, copy, save, reload and view toggles.
use super::super::{Control, control};
use crate::{
    files::{Files, Preview, pending::basename},
    root::Root,
    theme::Palette,
};
use gpui::{Context, div, prelude::*, px};

pub(super) fn file_toolbar(
    root: &Root,
    files: &Files,
    path: &str,
    preview: Option<&crate::files::text_preview::TextPreview>,
    palette: Palette,
    cx: &mut Context<Root>,
) -> gpui::Div {
    let dirty = files.editor.as_ref().is_some_and(|editor| editor.dirty(cx))
        || files
            .image_editor
            .as_ref()
            .is_some_and(crate::files::image_editor::ImageEditor::dirty);
    let saving = files.saving();
    let path = path.to_owned();
    let label = format!("{}{}", basename(&path), if dirty { " *" } else { "" });
    div()
        .h(px(35.))
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
                .id("file-document-path")
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(10.))
                .text_color(palette.muted)
                .tooltip(crate::assets::tooltip(path.clone(), palette))
                .child(label),
        )
        .when(files.opened_path().is_some(), |bar| {
            bar.child(
                Control::CopyPath.wrap(
                    root,
                    "Copy file path",
                    true,
                    None,
                    control("copy-file-path", "copy", palette)
                        .tooltip(crate::assets::tooltip("Copy file path", palette))
                        .on_click(cx.listener(|root, _, _, cx| root.copy_open_file_path(cx))),
                ),
            )
        })
        .when_some(files.editor.as_ref(), |bar, editor| {
            bar.child(
                Control::Save.wrap(
                    root,
                    "Save file",
                    dirty && !files.busy() && !editor.reload_required,
                    None,
                    control("save-file", "save", palette)
                        .tooltip(crate::assets::tooltip(
                            if saving {
                                "Saving file…"
                            } else {
                                "Save file"
                            },
                            palette,
                        ))
                        .when(!dirty || saving || editor.reload_required, |button| {
                            button.opacity(0.4)
                        })
                        .on_click(cx.listener(|root, _, _, cx| root.save_file(cx))),
                ),
            )
        })
        .child(
            control("reload-file", "reload", palette)
                .tooltip(crate::assets::tooltip("Reload file", palette))
                .when(saving, |button| button.opacity(0.4))
                .on_click(cx.listener(|root, _, _, cx| root.reload_file(cx))),
        )
        .when(!files.backend.is_local(), |bar| {
            bar.child(super::super::transfer_view::inline(
                root, files, false, palette, cx,
            ))
        })
        .when(files.backend.is_local(), |bar| {
            bar.child(
                control("show-file-explorer", "folder", palette)
                    .tooltip(crate::assets::tooltip(
                        "Show file in system explorer",
                        palette,
                    ))
                    .on_click(cx.listener(|root, _, _, cx| root.open_file_explorer(true, cx))),
            )
        })
        .when(
            files.editor.is_some() && preview.is_none_or(|preview| !preview.showing),
            |bar| {
                bar.child(
                    control("wrap-file-lines", "wrap", palette)
                        .when(files.wrap, |button| {
                            button.bg(palette.hover).text_color(palette.accent)
                        })
                        .tooltip(crate::assets::tooltip(
                            if files.wrap {
                                "Disable line wrapping"
                            } else {
                                "Wrap long lines"
                            },
                            palette,
                        ))
                        .on_click(cx.listener(|root, _, _, cx| {
                            if let Some(files) = &mut root.files.panel {
                                files.wrap = !files.wrap;
                                cx.notify();
                            }
                        })),
                )
            },
        )
        .when(
            files.preview.as_ref().is_some_and(Preview::editable),
            |bar| {
                let editing = files.image_editor.is_some();
                bar.child(
                    control(
                        "toggle-image-editing",
                        if editing { "eye" } else { "edit" },
                        palette,
                    )
                    .when(editing, |button| {
                        button.bg(palette.hover).text_color(palette.accent)
                    })
                    .tooltip(crate::assets::tooltip(
                        if editing {
                            "Preview image"
                        } else {
                            "Edit image"
                        },
                        palette,
                    ))
                    .on_click(cx.listener(|root, _, _, cx| root.toggle_image_editor(cx))),
                )
            },
        )
        .when_some(preview, |bar, preview| {
            bar.child(preview_button(root, false, preview.showing, palette, cx))
                .child(preview_button(root, true, preview.showing, palette, cx))
        })
}

fn preview_button(
    root: &Root,
    show: bool,
    showing: bool,
    palette: Palette,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let (id, icon, label) = if show {
        ("file-preview", "eye", "Preview file")
    } else {
        ("file-source", "edit", "Edit source")
    };
    let kind = if show {
        Control::Preview
    } else {
        Control::Source
    };
    kind.wrap(
        root,
        label,
        true,
        None,
        control(id, icon, palette)
            .when(show == showing, |button| {
                button.bg(palette.hover).text_color(palette.accent)
            })
            .tooltip(crate::assets::tooltip(label, palette))
            .on_click(cx.listener(move |root, _, _, cx| root.set_file_preview(show, cx))),
    )
}
