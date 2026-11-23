use crate::{
    assets::icon,
    files::{
        pending::FileAction,
        resizer::{ResizeAxis, files_resizer},
    },
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Div, Stateful, Window, div, prelude::*, px};
mod browser;
mod controls;
use controls::Control;
pub(crate) mod confirmation;
mod document;
pub(super) mod picker;
mod transfer_view;

fn control(
    id: impl Into<gpui::SharedString>,
    name: &'static str,
    palette: Palette,
) -> Stateful<Div> {
    icon_button(id, name, palette, true)
}

fn icon_button(
    id: impl Into<gpui::SharedString>,
    name: &'static str,
    palette: Palette,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .when(enabled, |style| {
            style
                .tab_index(0)
                .cursor_pointer()
                .hover(|style| style.bg(palette.hover).text_color(palette.accent))
        })
        .when(!enabled, |style| style.opacity(0.4))
        .border_1()
        .border_color(gpui::transparent_black())
        .focus(|style| style.border_color(palette.accent))
        .rounded(px(2.))
        .w(px(24.))
        .h(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_color(palette.muted)
        .child(icon(name).size(px(match name {
            "save" | "reload" | "maximize" | "restore" | "wrap" => 14.,
            "file-upload" | "file-download" => 15.,
            _ => 16.,
        })))
}
fn text_button(id: &'static str, label: &'static str, palette: Palette) -> Stateful<Div> {
    text_button_enabled(id, label, palette, true)
}

fn text_button_enabled(
    id: &'static str,
    label: &'static str,
    palette: Palette,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .when(enabled, |style| {
            style
                .tab_index(0)
                .cursor_pointer()
                .hover(|style| style.bg(palette.hover))
        })
        .when(!enabled, |style| style.opacity(0.4))
        .focus(|style| style.border_color(palette.accent))
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
        .child(label)
}
impl Root {
    pub(crate) fn with_peer_files(
        &self,
        terminal: impl IntoElement,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = self
            .files
            .as_ref()
            .is_some_and(|f| f.visible && f.source == self.peers.selected);
        div()
            .relative()
            .flex()
            .size_full()
            .child(terminal)
            .when(shown, |view| {
                let panel = self.files_panel(palette, window, cx);
                if self.files_narrow(window) {
                    view.child(div().absolute().inset_0().child(panel))
                } else {
                    view.child(panel)
                }
            })
            .into_any_element()
    }

    pub(crate) fn files_panel(
        &self,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(files) = self.files.as_ref().filter(|files| files.visible) else {
            return div().into_any_element();
        };
        if !self.files_source_current() {
            return div().p(px(8.)).flex().flex_col().gap(px(6.))
                .child("Document source retired. The original draft is retained; it will not be saved to the selected source.")
                .when_some(files.document_error.clone(), |panel, error| panel.child(error))
                .when(self.can_resume_file_draft(cx), |panel| {
                    panel.child(
                        text_button("resume-file-draft", "Resume draft", palette)
                            .on_click(cx.listener(|root, _, _, cx| root.resume_file_draft(cx))),
                    )
                })
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
            .track_focus(&files.panel_focus)
            .occlude()
            .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key != "tab" {
                    return;
                }
                if event.keystroke.modifiers.shift {
                    window.focus_prev();
                } else {
                    window.focus_next();
                }
                window.prevent_default();
                cx.stop_propagation();
            })
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
            panel = panel.child(files_resizer(
                ResizeAxis::Width,
                palette,
                &files.width_focus,
                cx,
            ));
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
            .child(Control::Browser.wrap(
                self, "Toggle file browser", true, Some(&files.browser_focus.toggle),
                control(
                    if files.browser {
                        "collapse-file-browser"
                    } else {
                        "expand-file-browser"
                    },
                    if files.browser { "up" } else { "down" },
                    palette,
                )
                .track_focus(&files.browser_focus.toggle)
                .bg(if files.browser {
                    palette.hover
                } else {
                    palette.surface
                })
                .border_color(palette.border)
                .when(files.browser, |style| style.text_color(palette.accent))
                .tooltip(crate::assets::tooltip(
                    format!(
                        "{} file browser ({})",
                        if files.browser { "Collapse" } else { "Expand" },
                        if cfg!(target_os = "macos") {
                            "⌘⇧E"
                        } else {
                            "Ctrl+Shift+E"
                        },
                    ),
                    palette,
                ))
                .on_click(cx.listener(|root, _, window, cx| root.toggle_file_browser(window, cx))),
            ))
            .child(div().flex_1())
            .when(files.backend.is_local(), |header| {
                header.child(
                    control("open-folder-explorer", "folder", palette)
                        .tooltip(crate::assets::tooltip("Open in system explorer", palette))
                        .on_click(cx.listener(|root, _, _, cx| root.open_file_explorer(false, cx))),
                )
            });
        if !narrow || files.maximized {
            header = header.child(Control::Maximize.wrap(
                self, if files.maximized { "Restore file" } else { "Maximize file" }, true, None,
                control(
                    if files.maximized {
                        "restore-file"
                    } else {
                        "maximize-file"
                    },
                    if files.maximized {
                        "restore"
                    } else {
                        "maximize"
                    },
                    palette,
                )
                .when(files.maximized, |style| {
                    style.bg(palette.hover).text_color(palette.accent)
                })
                .tooltip(crate::assets::tooltip(
                    if files.maximized {
                        "Restore file"
                    } else {
                        "Maximize file"
                    },
                    palette,
                ))
                .on_click(
                    cx.listener(|root, _, window, cx| root.toggle_file_maximized(window, cx)),
                ),
            ));
        }
        header = header.child(Control::Close.wrap(
            self, "Close file sidebar", true, None,
            text_button("hide-files", "×", palette)
                .w(px(24.))
                .tooltip(crate::assets::tooltip("Close file sidebar", palette))
                .on_click(
                    cx.listener(|root, _, _, cx| root.request_file_action(FileAction::Hide, cx)),
                ),
        ));
        panel = panel.child(header);
        if files.browser {
            panel = panel.child(self.file_browser(palette, window, cx));
        }
        let document_width = width - if files.maximized || narrow { 0. } else { 5. };
        panel = panel.child(self.file_document(palette, document_width, window, cx));
        if let Some(confirmation) = self.file_confirmation(palette, window, cx) {
            panel = panel.child(confirmation);
        }
        self.semantic_region(
            "root/files",
            crate::accessibility::Role::Pane,
            "File sidebar",
            panel,
        )
    }
}
