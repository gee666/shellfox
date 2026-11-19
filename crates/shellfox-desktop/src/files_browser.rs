use super::{control, text_button};
use crate::{
    files::{
        Files,
        resizer::{ResizeAxis, files_resizer},
    },
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
#[path = "files_browser_rows.rs"]
mod rows;
use rows::{Rows, message};

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
        let height = ResizeAxis::Height.clamp(self, window, files.browser_height);
        let parent = files
            .listing
            .as_ref()
            .and_then(|listing| listing.parent_path.clone());
        let busy = files.busy && files.operation.is_none();
        let home = if files.backend.is_local() {
            std::env::home_dir().and_then(|path| path.into_os_string().into_string().ok())
        } else {
            self.peers
                .selected_peer()
                .and_then(|(_, peer)| peer.home.clone())
        }
        .unwrap_or_else(|| "~".into());
        let focused = files.location.read(cx).is_focused(window);
        let match_list = files
            .completion
            .state
            .visible(&files.location.read(cx).text);
        let line = div()
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
            .border_color(if focused || files.browse_focus.is_focused(window) {
                palette.accent
            } else {
                palette.input_border
            })
            .rounded(px(2.))
            .child(
                text_button("parent-folder", "↑", palette)
                    .when(parent.is_none() || busy, |style| style.opacity(0.4))
                    .tooltip(crate::assets::tooltip("Parent folder", palette))
                    .on_click(cx.listener(move |root, _, _, cx| {
                        if let Some(path) = &parent {
                            root.list_files(path.clone(), cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("files-host-home")
                    .text_color(palette.accent)
                    .tooltip(crate::assets::tooltip(home, palette))
                    .child("~"),
            )
            .child(div().text_color(palette.accent).child("›"))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .on_key_down(cx.listener(Self::file_path_key))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| root.reopen_file_completion(cx)),
                    )
                    .child(files.location.clone()),
            )
            .when(files.backend.is_local(), |line| {
                line.child(
                    control("browse-files", "folder", palette)
                        .track_focus(&files.browse_focus)
                        .tab_stop(true)
                        .tooltip(crate::assets::tooltip("Browse folders", palette))
                        .on_click(cx.listener(|root, _, window, cx| root.browse_files(window, cx))),
                )
            })
            .child(
                text_button("files-go", "Go", palette)
                    .tooltip(crate::assets::tooltip("Open path", palette))
                    .on_click(cx.listener(|root, _, _, cx| root.submit_file_path(cx))),
            )
            .child(
                text_button("refresh-files", "↻", palette)
                    .tooltip(crate::assets::tooltip("Refresh files", palette))
                    .on_click(cx.listener(|root, _, _, cx| {
                        if let Some(path) = root.files.as_ref().and_then(|files| {
                            files.listing.as_ref().map(|listing| listing.path.clone())
                        }) {
                            root.list_files(path, cx);
                        }
                    })),
            )
            .when(
                !files.backend.is_local() && files.document_path.is_none(),
                |line| line.child(super::transfer_view::inline(files, true, palette, cx)),
            );
        let mut tree = Rows::new(files, palette);
        if files.listing.is_none() && busy {
            tree.push(message("Loading folder…", palette));
        }
        if let Some((path, error)) = &files.listing_error {
            let path = path.clone();
            let retry = files.listing.is_none();
            tree.push(message(error.clone(), palette).when(retry, |message| {
                message.child(
                    text_button("retry-directory", "Retry folder", palette).on_click(
                        cx.listener(move |root, _, _, cx| root.list_files(path.clone(), cx)),
                    ),
                )
            }));
        }
        if let Some(listing) = &files.listing {
            tree.listing(listing, 0, cx);
        }
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(
                div()
                    .relative()
                    .capture_key_down(cx.listener(|root, _: &gpui::KeyDownEvent, _, cx| {
                        set_keyboard_focus(root, true, cx)
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| set_keyboard_focus(root, false, cx)),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(|root, _, _, cx| set_keyboard_focus(root, false, cx)),
                    )
                    .h(px(height))
                    .min_h_0()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .py(px(3.))
                            .px(px(4.))
                            .border_b_1()
                            .border_color(palette.border)
                            .child(line),
                    )
                    .when(files.missing_folder.is_some(), |browser| {
                        browser.child(missing_folder(files, palette, window, cx))
                    })
                    .child(tree.render())
                    .when(
                        focused
                            && !busy
                            && files.missing_folder.is_none()
                            && !match_list.is_empty(),
                        |browser| {
                            browser.child(completion_list(match_list, files, palette, window, cx))
                        },
                    ),
            )
            .child(files_resizer(
                ResizeAxis::Height,
                palette,
                &files.height_focus,
                cx,
            ))
            .into_any_element()
    }
}

fn set_keyboard_focus(root: &mut Root, visible: bool, cx: &mut Context<Root>) {
    if let Some(files) = &mut root.files
        && files.tree.keyboard_focus != visible
    {
        files.tree.keyboard_focus = visible;
        cx.notify();
    }
}

fn completion_list(
    matches: &[String],
    files: &Files,
    palette: Palette,
    window: &Window,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let active = files.completion.state.active();
    let max_height = (f32::from(window.viewport_size().height) * 0.35).min(240.);
    div()
        .id("file-directory-completions")
        .absolute()
        .top(px(35.))
        .left(px(4.))
        .right(px(4.))
        .occlude()
        .track_scroll(&files.completion.scroll)
        .overflow_y_scroll()
        .max_h(px(max_height))
        .p(px(2.))
        .border_1()
        .border_color(palette.input_border)
        .rounded(px(2.))
        .bg(palette.input_bg)
        .font_family("Cascadia Code")
        .text_size(px(12.))
        .line_height(px(20.))
        .children(matches.iter().enumerate().map(|(index, value)| {
            div()
                .id(("file-match", index))
                .flex()
                .items_center()
                .h(px(26.))
                .gap(px(6.))
                .px(px(6.))
                .border_1()
                .border_color(gpui::transparent_black())
                .when(index == active, |style| {
                    style
                        .bg(palette.accent.opacity(0.12))
                        .border_color(palette.accent)
                })
                .hover(|style| style.bg(palette.hover))
                .cursor_pointer()
                .child(
                    crate::assets::icon("folder")
                        .size(px(16.))
                        .flex_shrink_0()
                        .text_color(palette.muted),
                )
                .child(div().min_w_0().truncate().child(value.clone()))
                .on_click(cx.listener(move |root, _, window, cx| {
                    root.choose_file_completion(index, window, cx)
                }))
        }))
}

fn missing_folder(
    files: &Files,
    palette: Palette,
    window: &Window,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let focus = &files.missing_focus;
    if !focus.iter().any(|handle| handle.is_focused(window)) {
        let keep = focus[0].clone();
        window.defer(cx, move |window, _| window.focus(&keep));
    }
    div()
        .m(px(4.))
        .pt(px(12.))
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap(px(12.))
        .border_t_1()
        .border_color(palette.border)
        .on_key_down(cx.listener(Root::file_path_key))
        .child(
            div().child("This folder doesn't exist. Create it?").child(
                div()
                    .mt(px(4.))
                    .text_size(px(11.))
                    .child("Parent folders will be created too."),
            ),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(7.))
                .child(
                    text_button("keep-editing-file-path", "Keep editing", palette)
                        .track_focus(&focus[0])
                        .tab_stop(true)
                        .h(px(28.))
                        .px(px(10.))
                        .focus(|style| style.border_color(palette.accent))
                        .on_click(cx.listener(|root, _, window, cx| keep_editing(root, window, cx)))
                        .on_key_down(cx.listener(
                            |root, event: &gpui::KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    keep_editing(root, window, cx);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                }
                            },
                        )),
                )
                .child(
                    text_button("create-file-folder", "Create folder", palette)
                        .track_focus(&focus[1])
                        .tab_stop(true)
                        .h(px(28.))
                        .px(px(10.))
                        .focus(|style| style.border_color(palette.accent))
                        .on_click(
                            cx.listener(|root, _, _, cx| root.confirm_missing_file_folder(cx)),
                        )
                        .on_key_down(cx.listener(
                            |root, event: &gpui::KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    root.confirm_missing_file_folder(cx);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                }
                            },
                        )),
                ),
        )
}

fn keep_editing(root: &mut Root, window: &mut Window, cx: &mut Context<Root>) {
    if let Some(files) = &mut root.files {
        files.missing_folder = None;
        files.location.read(cx).focus(window);
        cx.notify();
    }
}
