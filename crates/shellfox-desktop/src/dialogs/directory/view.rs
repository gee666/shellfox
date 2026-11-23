use crate::{
    assets::{ellipsis, icon, icon_button, tooltip},
    dialogs::{
        dialog_button,
        directory::{Busy, Draft},
    },
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

impl Root {
    pub(crate) fn render_directory(
        &self,
        draft: &Draft,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let locked = self.dialog_locked();
        let peer = draft.peer_name();
        let focused =
            draft.path.read(cx).edit_focus().is_focused(window) || draft.browse.is_focused(window);
        let mut path = div().relative().h(px(36.)).child(
            div()
                .h(px(36.))
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(10.))
                .pr(px(5.))
                .py(px(3.))
                .bg(palette.input_bg)
                .border_1()
                .border_color(if focused {
                    palette.accent
                } else {
                    palette.input_border
                })
                .rounded(px(2.))
                .font_family(directory_font())
                .text_size(px(13.))
                .line_height(px(22.))
                .child(
                    div()
                        .id("directory-home")
                        .text_color(palette.accent)
                        .tooltip(tooltip(
                            draft
                                .home
                                .as_ref()
                                .map_or_else(|| "~".into(), |home| home.display().to_string()),
                            palette,
                        ))
                        .child("~"),
                )
                .child(div().text_color(palette.accent).child("›"))
                .child(
                    div()
                        .id("directory-input-slot")
                        .flex_1()
                        .min_w_0()
                        .child(draft.path.clone())
                        .on_click(cx.listener(|root, _, _, cx| root.reopen_directory_matches(cx))),
                )
                .when(peer.is_none(), |line| {
                    line.child(
                        icon_button("browse-directory", "folder", palette)
                            .track_focus(&draft.browse)
                            .tooltip(tooltip("Browse folders", palette))
                            .focus(|style| style.border_1().border_color(palette.accent))
                            .when(locked, |button| button.opacity(0.4))
                            .on_click(
                                cx.listener(|root, _, window, cx| {
                                    root.browse_directory(window, cx)
                                }),
                            ),
                    )
                }),
        );
        let options = draft.matches(cx);
        if !options.is_empty() {
            let max_height = 180_f32.min(f32::from(window.viewport_size().height) * 0.3);
            let natural_height = 6. + 26. * options.len() as f32;
            let scrolling = natural_height > max_height;
            // Suggestions outside the card still belong to the picker, not
            // the dismissible modal backdrop behind the deferred list.
            let viewport = div().id("directory-completions")
                .occlude()
                .absolute()
                .top(px(38.))
                .left_0()
                .right_0()
                .h(px(natural_height.min(max_height)))
                .border_1()
                .border_color(palette.input_border)
                .rounded(px(2.))
                .bg(palette.input_bg)
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::Hsla::from(gpui::rgb(0)).opacity(0.4),
                    offset: gpui::point(px(0.), px(4.)),
                    blur_radius: px(10.),
                    spread_radius: px(0.),
                }]);
            let mut matches = div().id("directory-completions-scroll").h_full()
                .overflow_y_scroll().track_scroll(&draft.scroll).p(px(2.))
                .when(scrolling, |list| list.mr(px(15.)));
            for (index, value) in options.iter().enumerate() {
                let selected = index == draft.active();
                matches = matches.child(
                    div()
                        .id(("directory-match", index))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(5.))
                        .py(px(2.))
                        .font_family(directory_font())
                        .text_size(px(12.))
                        .line_height(px(20.))
                        .text_color(palette.foreground)
                        .cursor_pointer()
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .when(selected, |style| {
                            style
                                .bg(palette.accent.opacity(0.12))
                                .border_color(palette.accent)
                        })
                        .when(!selected, |style| {
                            style.hover(|style| style.bg(palette.hover))
                        })
                        .tooltip(tooltip(value.clone(), palette))
                        .child(icon("folder").flex_shrink_0().text_color(palette.muted))
                        .child(div().flex_1().min_w_0().child(ellipsis(value.clone(), 20.)))
                        .on_click(cx.listener(move |root, _, _, cx| {
                            root.choose_directory_match(Some(index), cx);
                        })),
                );
            }
            let viewport = viewport.child(matches).when(scrolling, |panel| {
                panel.child(draft.scrollbar.render(&draft.scroll, palette))
            });
            path = path.child(gpui::deferred(viewport).with_priority(60));
        }
        let mut body = div()
            .p(px(12.))
            .child(
                div()
                    .h(px(15.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .mb(px(12.))
                    .text_size(px(11.))
                    .child(
                        crate::assets::icon_sized(
                            if peer.is_some() { "server" } else { "monitor" },
                            13.,
                        )
                        .text_color(palette.muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .child(peer.unwrap_or("Local").to_owned()),
                    )
                    .when(peer.is_some(), |line| {
                        line.child(
                            div()
                                .text_size(px(10.))
                                .text_color(palette.muted)
                                .relative()
                                .left(px(1.))
                                .top(px(1.))
                                .child("Remote folder"),
                        )
                    }),
            )
            .child(path)
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(10.))
                    .line_height(px(15.))
                    .text_color(palette.muted)
                    .child(match draft.busy {
                        Some(Busy::Creating) => "Creating folder…",
                        Some(Busy::Checking) => "Checking folder…",
                        None if self.user_action_pending => "Checking folder…",
                        None if peer.is_some() => {
                            "Tab complete · ↑↓ select · Enter accept/open · Remote folder"
                        }
                        None => "Tab complete · ↑↓ select · Enter accept/open · Shift+Tab browse",
                    }),
            );
        if draft.missing.is_some() {
            body = body.child(
                div()
                    .mt(px(14.))
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(palette.border)
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .line_height(px(16.))
                                    .child("This folder doesn't exist. Create it?"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(11.))
                                    .line_height(px(15.))
                                    .text_color(palette.muted)
                                    .child("Parent folders will be created too."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(7.))
                            .child(
                                dialog_button("directory-keep-editing", "Keep editing", palette)
                                    .track_focus(&draft.keep_editing)
                                    .h(px(28.))
                                    .px(px(10.))
                                    .text_size(px(13.))
                                    .when(draft.keep_editing.is_focused(window), |button| {
                                        button.border_color(palette.accent)
                                    })
                                    .when(locked, |b| b.opacity(0.4))
                                    .on_click(cx.listener(Self::directory_keep_editing)),
                            )
                            .child(
                                div()
                                    .id("directory-create-folder")
                                    .track_focus(&draft.create_missing)
                                    .h(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .px(px(10.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .bg(palette.accent.opacity(0.08))
                                    .text_color(palette.accent)
                                    .border_color(palette.accent)
                                    .hover(|style| style.bg(palette.accent.opacity(0.16)))
                                    .child("Create folder")
                                    .when(locked, |b| b.opacity(0.4))
                                    .on_click(cx.listener(|root, _, window, cx| {
                                        root.submit_directory(
                                            Some(Busy::Creating),
                                            None,
                                            window,
                                            cx,
                                        )
                                    })),
                            ),
                    ),
            );
        }
        if let Some(error) = &draft.error {
            body = body.child(
                div()
                    .mt(px(4.))
                    .text_color(palette.status_color(0xfc8397))
                    .child(error.clone()),
            );
        }
        self.modal_frame("New session", body.into_any_element(), palette, cx)
    }

    fn directory_keep_editing(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_directory(window, cx);
    }
}

fn directory_font() -> &'static str {
    if cfg!(windows) {
        "Cascadia Code"
    } else {
        "monospace"
    }
}
