use crate::{assets::icon, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Div, Stateful, Window, div, prelude::*, px};
#[path = "files_browser.rs"]
mod browser;
#[path = "files_document.rs"]
mod document;

fn control(
    id: impl Into<gpui::SharedString>,
    name: &'static str,
    palette: Palette,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .w(px(24.))
        .h(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_color(palette.muted)
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover).text_color(palette.accent))
        .child(icon(name).size(px(
            if matches!(name, "save" | "reload" | "maximize" | "restore") {
                14.
            } else {
                16.
            },
        )))
}
fn text_button(id: &'static str, label: &'static str, palette: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(24.))
        .px(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .rounded(px(2.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface)
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover))
        .child(label)
}
impl Root {
    pub(crate) fn files_panel(
        &self,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(files) = self.files.as_ref().filter(|files| files.visible) else {
            return div().into_any_element();
        };
        if !self.files_source_current() {
            return div().p(px(8.)).flex().flex_col().gap(px(6.))
                .child("Document source retired. The original draft is retained; it will not be saved to the selected source.")
                .when(files.busy,|panel|panel.child(text_button("cancel-retired-files","Cancel original operation",palette).on_click(cx.listener(|root,_,_,cx|root.cancel_file_operation(cx)))))
                .when(!files.busy,|panel|panel.child(text_button("discard-retired-files","Discard old draft and switch Files",palette).on_click(cx.listener(|root,_,_,cx|root.discard_retired_files(cx)))))
                .into_any_element();
        }
        let narrow = self.files_narrow(window);
        let max = (self.workspace_width(window) - 200.).max(0.);
        let width = if files.maximized {
            f32::from(window.viewport_size().width)
        } else if narrow {
            self.workspace_width(window)
        } else {
            files.width.clamp(300f32.min(max), max)
        };
        let mut panel = div()
            .relative()
            .occlude()
            .w(px(width))
            .h_full()
            .min_w_0()
            .min_h_0()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(palette.panel)
            .text_size(px(12.))
            .when(!files.maximized && !narrow, |style| {
                style.pl(px(4.)).border_l_1().border_color(palette.border)
            });
        if !files.maximized && !narrow {
            panel = panel.child(
                div()
                    .id("file-width-resizer")
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(4.))
                    .border_r_1()
                    .border_color(palette.border)
                    .cursor(gpui::CursorStyle::ResizeColumn)
                    .hover(|style| style.bg(palette.accent))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            if let Some(files) = &mut root.files {
                                files.resizing_width = true;
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            if let Some(files) = &mut root.files {
                                files.resizing_width = false;
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_up_out(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            if let Some(files) = &mut root.files {
                                files.resizing_width = false;
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        let mut header = div()
            .h(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(4.))
            .bg(palette.sidebar)
            .border_b_1()
            .border_color(palette.border)
            .child(
                control(
                    "file-browser-toggle",
                    if files.browser { "up" } else { "down" },
                    palette,
                )
                .when(files.browser, |style| {
                    style.bg(palette.hover).text_color(palette.accent)
                })
                .on_click(cx.listener(|root, _, _, cx| root.toggle_file_browser(cx))),
            )
            .child(div().flex_1())
            .child(
                control("open-folder-explorer", "folder", palette)
                    .on_click(cx.listener(|root, _, _, cx| root.open_file_explorer(false, cx))),
            );
        if !narrow || files.maximized {
            header = header.child(
                control(
                    "maximize-file",
                    if files.maximized {
                        "restore"
                    } else {
                        "maximize"
                    },
                    palette,
                )
                .on_click(
                    cx.listener(|root, _, window, cx| root.toggle_file_maximized(window, cx)),
                ),
            );
        }
        header = header.child(
            text_button("hide-files", "×", palette)
                .w(px(24.))
                .on_click(cx.listener(|root, _, _, cx| root.toggle_files(cx))),
        );
        panel = panel.child(header);
        if files.browser {
            panel = panel.child(self.file_browser(palette, window, cx));
        }
        panel = panel.child(self.file_document(palette, cx));
        if files.pending_open.is_some() {
            panel = panel.child(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .bg(palette.panel)
                    .child("Discard unsaved changes?")
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(4.))
                            .mt(px(12.))
                            .child(text_button("cancel-open-file", "Cancel", palette).on_click(
                                cx.listener(|root, _, _, cx| {
                                    if let Some(files) = &mut root.files {
                                        files.pending_open = None;
                                    }
                                    cx.notify();
                                }),
                            ))
                            .child(
                                text_button("discard-open-file", "Discard changes", palette)
                                    .on_click(cx.listener(|root, _, _, cx| {
                                        if let Some(path) = root.files.as_mut().and_then(|files| {
                                            if files.busy {
                                                None
                                            } else {
                                                files.pending_open.take()
                                            }
                                        }) {
                                            root.open_file(path, cx);
                                        }
                                    })),
                            ),
                    ),
            );
        }
        self.semantic_region(
            "root/files",
            crate::accessibility::Role::Pane,
            "File sidebar",
            panel,
        )
    }
}
