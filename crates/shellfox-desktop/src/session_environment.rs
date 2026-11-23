use crate::{
    dialogs::dialog_button,
    field::{Field, FieldOptions},
    root::{Dialog, Root},
    settings::environment_data,
    theme::Palette,
};
use gpui::{AnyElement, Context, Entity, Window, div, prelude::*, px};
use shellfox_core::SessionId;
pub(crate) mod resize;

impl Root {
    pub(super) fn edit_session_environment(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
        else {
            return;
        };
        let text = Field::textarea(environment_data::format(&session.env), cx);
        text.update(cx, |field, cx| {
            field.configure(
                FieldOptions {
                    placeholder: Some(
                        "# One per line\nAPI_KEY=sk-...\nNODE_ENV=development".into(),
                    ),
                    ..Default::default()
                },
                cx,
            )
        });
        cx.observe(&text, |_, _, cx| cx.notify()).detach();
        text.read(cx).focus(window);
        self.sidebar_ui.environment_resize = Default::default();
        self.dialog = Some(Dialog::SessionEnvironment { id, text });
        cx.notify();
    }

    pub(crate) fn session_environment_dialog(
        &self,
        id: SessionId,
        text: &Entity<Field>,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(session) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
        else {
            return div().into_any_element();
        };
        self.render_session_environment(
            session.title.clone(),
            text,
            cfg!(windows),
            palette,
            window,
            cx,
        )
    }

    pub(crate) fn render_session_environment(
        &self,
        title: String,
        text: &Entity<Field>,
        windows: bool,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (_, issues) = environment_data::parse(&text.read(cx).text, windows);
        let disabled = self.dialog_locked() || !issues.is_empty();
        let mut feedback = div()
            .mt(px(8.))
            .min_h(px(16.))
            .text_size(px(11.))
            .line_height(px(16.))
            .text_color(palette.muted)
            .child("Applies to new terminals in this session.");
        if !issues.is_empty() {
            let mut errors = div()
                .id("session-env-errors")
                .max_h(px(64.))
                .overflow_y_scroll();
            for issue in issues {
                errors = errors.child(
                    div()
                        .mt(px(4.))
                        .text_size(px(11.))
                        .text_color(palette.status_color(0xfc8397))
                        .child(format!("Line {}: {}", issue.line, issue.message)),
                );
            }
            feedback = feedback.child(errors);
        }
        let body = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .h(px(self.sidebar_ui.environment_resize.height(window)))
                    .border_1()
                    .rounded(px(4.))
                    .bg(palette.input_bg)
                    .border_color(if text.read(cx).edit_focus().is_focused(window) {
                        palette.accent
                    } else {
                        palette.input_border
                    })
                    .child(
                        div()
                            .id("session-environment-text")
                            .size_full()
                            .overflow_y_scroll()
                            .child(text.clone()),
                    )
                    .child(self.session_environment_handle(palette, cx)),
            )
            .child(feedback)
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        dialog_button("session-env-cancel", "Cancel", palette)
                            .h(px(27.))
                            .text_size(px(13.))
                            .line_height(px(17.))
                            .on_click(
                                cx.listener(|root, _, window, cx| root.dismiss_dialog(window, cx)),
                            ),
                    )
                    .child(
                        dialog_button("session-env-save", "Save", palette)
                            .h(px(27.))
                            .text_size(px(13.))
                            .line_height(px(17.))
                            .tooltip(crate::assets::tooltip("Ctrl+Enter", palette))
                            .when(disabled, |button| button.opacity(0.4))
                            .when(!disabled, |button| {
                                button.on_click(cx.listener(|root, _, window, cx| {
                                    root.confirm_dialog(window, cx)
                                }))
                            }),
                    ),
            );
        self.modal_frame(
            format!("Environment · {title}"),
            body.into_any_element(),
            palette,
            cx,
        )
    }
}
