use crate::{
    assets::{icon, icon_button},
    directory::Draft,
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, div, prelude::*, px};

impl Root {
    pub(crate) fn render_directory(
        &self,
        draft: &Draft,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
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
                .border_color(palette.accent)
                .rounded(px(2.))
                .font_family("Cascadia Code")
                .text_size(px(13.))
                .line_height(px(22.))
                .child(div().text_color(palette.accent).child("~"))
                .child(div().text_color(palette.accent).child("›"))
                .child(div().flex_1().min_w_0().child(draft.path.clone()))
                .child(icon_button("browse-directory", "folder", palette).on_click(
                    cx.listener(|root, _, window, cx| root.browse_directory(window, cx)),
                )),
        );
        if !draft.matches.is_empty() {
            let mut matches = div()
                .absolute()
                .top(px(38.))
                .left_0()
                .right_0()
                .max_h(px(180.))
                .overflow_hidden()
                .p(px(2.))
                .border_1()
                .border_color(palette.input_border)
                .rounded(px(2.))
                .bg(palette.input_bg);
            for (index, value) in draft.matches.iter().enumerate() {
                let selected = index == draft.active;
                matches = matches.child(
                    div()
                        .id(("directory-match", index))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(6.))
                        .py(px(3.))
                        .font_family("Cascadia Code")
                        .text_size(px(12.))
                        .line_height(px(20.))
                        .cursor_pointer()
                        .when(selected, |style| {
                            style
                                .bg(palette.accent.opacity(0.12))
                                .border_1()
                                .border_color(palette.accent)
                        })
                        .hover(|style| style.bg(palette.hover))
                        .child(icon("folder").text_color(palette.muted))
                        .child(div().truncate().child(value.clone()))
                        .on_click(cx.listener(move |root, _, window, cx| {
                            root.choose_directory_match(Some(index), window, cx)
                        })),
                );
            }
            path = path.child(matches);
        }
        let mut body = div()
            .p(px(12.))
            .on_key_down(cx.listener(Self::directory_key))
            .child(
                div()
                    .h(px(15.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .mb(px(12.))
                    .text_size(px(11.))
                    .child(icon("monitor").size(px(13.)).text_color(palette.muted))
                    .child("Local"),
            )
            .child(path)
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(10.))
                    .line_height(px(15.))
                    .text_color(palette.muted)
                    .child(if draft.busy {
                        "Checking folder…"
                    } else {
                        "Tab complete · ↑↓ select · Enter accept/open · Shift+Tab browse"
                    }),
            );
        if let Some(error) = &draft.error {
            body = body.child(
                div()
                    .mt_1()
                    .text_color(gpui::rgb(0xfc8397))
                    .child(error.clone()),
            );
        }
        self.modal_frame("New session", body.into_any_element(), palette, cx)
    }
}
