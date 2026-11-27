use super::{Control, icon_button, text_button, text_button_enabled};
use crate::{
    files::{
        Files,
        completion::completion_list,
        list::ListingView,
        resizer::{ResizeAxis, files_resizer},
    },
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
mod new_file;
mod rows;
use rows::{Rows, message, retry_message};

impl Root {
    pub(super) fn file_browser(
        &self,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(files) = &self.files.panel else {
            return div().into_any_element();
        };
        let height = ResizeAxis::Height.clamp(self, window, files.browser_height);
        let parent = files
            .listing
            .as_ref()
            .and_then(|listing| listing.parent_path.clone());
        files.completion.track_focus(&files.location, window, cx);
        let busy = files.busy();
        let loading =
            files.tree.loading || (files.listing.is_none() && busy && files.operation.is_none());
        let home = if files.backend.is_local() {
            std::env::home_dir().and_then(|path| path.into_os_string().into_string().ok())
        } else {
            self.peers
                .selected_peer()
                .and_then(|(_, peer)| peer.home.clone())
        }
        .unwrap_or_else(|| "~".into());
        let location = files.location.read(cx);
        let completion_focused = location.is_focused(window) && !location.is_composing();
        let match_list = files.completion.state.visible(&location.text);
        let line = div()
            .track_focus(&files.browser_focus.line)
            .tab_stop(false)
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
            .border_color(if files.browser_focus.line.contains_focused(window, cx) {
                palette.accent
            } else {
                palette.input_border
            })
            .rounded(px(2.))
            .child(
                text_button_enabled("parent-folder", "↑", palette, parent.is_some() && !busy)
                    .tooltip(crate::assets::tooltip("Parent folder", palette))
                    .when_some(parent.filter(|_| !busy), |button, path| {
                        button.on_click(
                            cx.listener(move |root, _, _, cx| root.list_files(path.clone(), cx)),
                        )
                    }),
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
                    Control::Browse.wrap(
                        self,
                        "Browse folders",
                        !busy,
                        Some(&files.browser_focus.browse),
                        icon_button("browse-files", "folder", palette, !busy)
                            .tooltip(crate::assets::tooltip("Browse folders", palette))
                            .when(!busy, |button| {
                                button.track_focus(&files.browser_focus.browse).on_click(
                                    cx.listener(|root, _, window, cx| {
                                        root.browse_files(window, cx)
                                    }),
                                )
                            }),
                    ),
                )
            })
            .child(
                Control::SubmitPath.wrap(
                    self,
                    "Open path",
                    !busy,
                    None,
                    text_button_enabled("files-go", "Go", palette, !busy)
                        .tooltip(crate::assets::tooltip("Open path", palette))
                        .when(!busy, |button| {
                            button.on_click(cx.listener(|root, _, _, cx| root.submit_file_path(cx)))
                        }),
                ),
            )
            .child(
                text_button_enabled("refresh-files", "↻", palette, !busy)
                    .tooltip(crate::assets::tooltip("Refresh files", palette))
                    .when(!busy, |button| {
                        button.on_click(cx.listener(|root, _, _, cx| {
                            if let Some(path) = root.files.panel.as_ref().and_then(|files| {
                                files
                                    .listing_error
                                    .as_ref()
                                    .and_then(|error| error.retry.clone())
                                    .or_else(|| {
                                        files.listing.as_ref().map(|listing| listing.path.clone())
                                    })
                            }) {
                                root.list_files(path, cx);
                            }
                        }))
                    }),
            )
            .child(new_file::button(self, files, palette, cx))
            .when(
                !files.backend.is_local() && files.document_path.is_none(),
                |line| line.child(super::transfer_view::inline(self, files, true, palette, cx)),
            );
        let mut tree = Rows::new(self, files, palette);
        match ListingView::new(
            loading,
            files.listing.as_ref(),
            files.listing_error.as_ref(),
        ) {
            ListingView::Loading => tree.push(message("Loading folder…", palette)),
            ListingView::Retry { message, path } => {
                let path = path.to_owned();
                tree.push(retry_message(
                    message.to_owned(),
                    palette,
                    "retry-directory",
                    cx.listener(move |root, _, _, cx| root.list_files(path.clone(), cx)),
                ));
            }
            ListingView::Rows {
                listing,
                message: notice,
            } => {
                if let Some(notice) = notice {
                    tree.push(message(notice.to_owned(), palette));
                }
                if let Some(listing) = listing {
                    tree.listing(listing, 0, cx);
                }
            }
        }
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(
                div()
                    .relative()
                    .track_focus(&files.browser_focus.region)
                    .tab_stop(false)
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
                    .children(new_file::form(files, palette, cx))
                    .when(files.missing_folder.is_some(), |browser| {
                        browser.child(missing_folder(files, palette, window, cx))
                    })
                    .child(tree.render())
                    .when(
                        completion_focused
                            && !busy
                            && files.missing_folder.is_none()
                            && !match_list.is_empty(),
                        |browser| {
                            browser.child(completion_list(
                                self, match_list, files, palette, window, cx,
                            ))
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
    if let Some(files) = root
        .files
        .panel
        .as_mut()
        .filter(|files| files.tree.keyboard_focus != visible)
    {
        files.tree.keyboard_focus = visible;
        cx.notify();
    }
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
    if let Some(files) = &mut root.files.panel {
        files.missing_folder = None;
        files.location.read(cx).focus(window);
        cx.notify();
    }
}
