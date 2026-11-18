use super::{control, text_button};
use crate::{root::Root, theme::Palette};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use shellfox_files::EntryKind;
impl Root {
    pub(super) fn file_browser(
        &self,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(files) = &self.files else {
            return div().into_any_element();
        };
        let max = (f32::from(window.viewport_size().height) - 200.).max(0.);
        let height = files.browser_height.clamp(68f32.min(max), max);
        let parent = files
            .listing
            .as_ref()
            .and_then(|listing| listing.parent_path.clone());
        let mut line = div()
            .h(px(30.))
            .flex()
            .items_center()
            .gap(px(3.))
            .px(px(3.))
            .font_family("Cascadia Code")
            .text_size(px(11.))
            .line_height(px(22.))
            .bg(palette.input_bg)
            .border_1()
            .border_color(palette.input_border)
            .rounded(px(2.))
            .on_key_down(cx.listener(Self::file_path_key))
            .child(
                text_button("parent-folder", "↑", palette)
                    .when(parent.is_none() || files.busy, |style| style.opacity(0.4))
                    .on_click(cx.listener(move |root, _, _, cx| {
                        if let Some(parent) = &parent {
                            root.list_files(parent.clone(), cx);
                        }
                    })),
            )
            .child(div().text_color(palette.accent).child("~"))
            .child(div().text_color(palette.accent).child("›"))
            .child(div().min_w_0().flex_1().child(files.location.clone()))
            .child(
                control("browse-files", "folder", palette)
                    .on_click(cx.listener(|root, _, window, cx| root.browse_files(window, cx))),
            )
            .child(text_button("files-go", "Go", palette).on_click(cx.listener(
                |root, _, _, cx| {
                    if let Some(files) = &root.files {
                        let path = files.location.read(cx).text.clone();
                        root.list_files(path, cx);
                    }
                },
            )));
        line = line.child(
            text_button("refresh-files", "↻", palette).on_click(cx.listener(|root, _, _, cx| {
                if let Some(path) = root
                    .files
                    .as_ref()
                    .and_then(|files| files.listing.as_ref().map(|listing| listing.path.clone()))
                {
                    root.list_files(path, cx);
                }
            })),
        );
        let mut browser = div()
            .h(px(height))
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .py(px(3.))
                    .px(px(4.))
                    .border_b_1()
                    .border_color(palette.border)
                    .child(line),
            );
        let mut tree = div().id("file-tree").flex_1().min_h_0().overflow_y_scroll();
        if files.busy && files.listing.is_none() {
            tree = tree.child(
                div()
                    .px(px(8.))
                    .py(px(5.))
                    .text_color(palette.muted)
                    .child("Loading folder…"),
            );
        }
        if let Some(listing) = &files.listing {
            if listing.entries.is_empty() {
                tree = tree.child(
                    div()
                        .px(px(8.))
                        .py(px(5.))
                        .text_color(palette.muted)
                        .child("Empty folder"),
                );
            }
            for (index, entry) in listing.entries.iter().enumerate() {
                let path = entry.path.clone();
                let kind = entry.kind;
                let selected = files
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.file.path == path);
                let supported = matches!(kind, EntryKind::File | EntryKind::Directory);
                tree = tree.child(
                    div()
                        .id(("file", index))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .gap(px(3.))
                        .px(px(8.))
                        .text_size(px(11.))
                        .when(selected, |style| {
                            style.bg(palette.hover).text_color(palette.accent)
                        })
                        .when(!supported, |style| style.opacity(0.4))
                        .cursor_pointer()
                        .hover(|style| style.bg(palette.hover))
                        .child(
                            div()
                                .w(px(12.))
                                .flex_shrink_0()
                                .text_center()
                                .text_color(palette.muted)
                                .child(if kind == EntryKind::Directory {
                                    "▸"
                                } else {
                                    "·"
                                }),
                        )
                        .child(div().truncate().child(entry.name.clone()))
                        .on_click(cx.listener(move |root, _, _, cx| {
                            if kind == EntryKind::Directory {
                                root.list_files(path.clone(), cx);
                            } else if kind == EntryKind::File {
                                root.request_open_file(path.clone(), cx);
                            }
                        })),
                );
            }
            if listing.truncated {
                tree = tree.child(
                    div()
                        .px(px(8.))
                        .py(px(5.))
                        .text_color(palette.muted)
                        .child("Folder listing truncated. Open a subfolder or enter its path."),
                );
            }
        }
        browser = browser.child(tree);
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(browser)
            .child(
                div()
                    .id("file-browser-resizer")
                    .h(px(4.))
                    .w_full()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_b_1()
                    .border_color(palette.border)
                    .cursor(gpui::CursorStyle::ResizeRow)
                    .hover(|style| style.bg(palette.accent))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            if let Some(files) = &mut root.files {
                                files.resizing_browser = true;
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            if let Some(files) = &mut root.files {
                                files.resizing_browser = false;
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_up_out(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            if let Some(files) = &mut root.files {
                                files.resizing_browser = false;
                            }
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element()
    }
}
