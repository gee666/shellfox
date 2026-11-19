use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn environment_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let viewport = window.viewport_size();
        let Some(environment) = &self.environment else {
            return div().into_any_element();
        };
        let field = environment.text.clone();
        let error = environment.error.clone();
        let pending = environment.pending;
        let parent = self.environment_semantic_id();
        let field = self.bind_field(field, "Environment variables", parent, cx);
        let empty = field.read(cx).text.is_empty();
        let focused = field.read(cx).is_focused(window);
        let (_, issues) = environment_data::parse(
            &field.read(cx).text,
            self.catalog
                .as_ref()
                .is_none_or(|catalog| catalog.platform == "win32"),
        );
        let invalid = pending || !issues.is_empty() || self.catalog.is_none();
        let close = self
            .text_button("close-environment", "", Action::CancelEnvironment, cx)
            .w(px(28.))
            .h(px(28.))
            .p_0()
            .child(crate::assets::icon("close"));
        let cancel = self.button(
            "cancel-environment",
            "Cancel",
            Action::CancelEnvironment,
            cx,
        );
        let save = self
            .button("save-environment", "Save", Action::SaveEnvironment, cx)
            .disabled(invalid)
            .when(invalid, |style| {
                style.opacity(0.4).cursor(gpui::CursorStyle::Arrow)
            });
        let mut feedback = div().mt(px(8.)).min_h(px(16.)).child(
            controls::hint(
                "Applies to new terminals on this host. Session variables override these values.",
                p,
            )
            .line_height(px(16.))
            .mb_0(),
        );
        if let Some(error) = error {
            feedback = feedback.child(
                controls::hint(error, p)
                    .mt(px(4.))
                    .text_color(gpui::rgb(0xfc8397)),
            );
        }
        let mut errors = div()
            .id("environment-validation-errors")
            .max_h(px(64.))
            .overflow_y_scroll();
        for issue in issues {
            errors = errors.child(
                div()
                    .mt(px(4.))
                    .text_size(px(11.))
                    .text_color(gpui::rgb(0xfc8397))
                    .child(format!("Line {}: {}", issue.line, issue.message)),
            );
        }
        feedback = feedback.child(errors);
        let body = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .h(px(180.))
                    .min_h(px(100.))
                    .overflow_hidden()
                    .bg(p.input_bg)
                    .border_1()
                    .border_color(if focused { p.accent } else { p.input_border })
                    .rounded(px(4.))
                    .child(field)
                    .when(empty, |textarea| {
                        textarea.child(
                            div()
                                .absolute()
                                .left(px(9.))
                                .top(px(8.))
                                .font_family("Cascadia Code")
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(p.placeholder)
                                .child(div().child("# One per line"))
                                .child(div().child("API_KEY=sk-..."))
                                .child(div().child("NODE_ENV=development")),
                        )
                    }),
            )
            .child(feedback)
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(cancel)
                    .child(save),
            );
        let card = div()
            .w(px(480.))
            .max_w_full()
            .max_h(viewport.height * 0.8)
            .flex()
            .flex_col()
            .bg(p.panel)
            .border_1()
            .border_color(p.border)
            .rounded(px(8.))
            .shadow_lg()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(16.))
                    .pr(px(12.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Environment · All terminals"),
                    )
                    .child(close),
            )
            .child(body);
        let card = self.semantic_dialog(card);
        let backdrop = div()
            .id("all-terminal-environment-modal")
            .w(viewport.width)
            .h(viewport.height)
            .p(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.53))
            .capture_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    panel.environment_key(event, window, cx)
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|panel, _, window, cx| panel.close_environment(window, cx)),
            )
            .child(card);
        let popup = gpui::anchored()
            .position(gpui::point(px(0.), px(0.)))
            .child(backdrop);
        match (&self.semantic, parent) {
            (Some(binding), Some(id)) => binding
                .frame
                .deferred_subtree(id, popup, 50)
                .into_any_element(),
            _ => gpui::deferred(popup).with_priority(50).into_any_element(),
        }
    }
}
