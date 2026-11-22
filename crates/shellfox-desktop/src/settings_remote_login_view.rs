use super::{
    controls::{self, Action},
    remote::Operation,
    *,
};

fn disclosure_marker(open: bool, color: gpui::Hsla) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut marker = gpui::PathBuilder::fill();
            let points = if open {
                [(0., 2.), (8., 2.), (4., 8.)]
            } else {
                [(1., 0.), (7., 4.), (1., 8.)]
            };
            let [a, b, c] = points.map(|(x, y)| bounds.origin + gpui::point(px(x), px(y)));
            marker.move_to(a);
            marker.line_to(b);
            marker.line_to(c);
            marker.close();
            if let Ok(path) = marker.build() {
                window.paint_path(path, color);
            }
        },
    )
    .absolute()
    .left_0()
    .top(px(4.))
    .size(px(8.))
}

impl SettingsPanel {
    pub(super) fn remote_login_controls(
        &mut self,
        mut account: gpui::Div,
        state: &api::RemoteState,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = self.palette;
        let mut actions = div()
            .mt(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .flex_wrap();
        let native_attempt = state
            .attempts
            .iter()
            .find(|attempt| matches!(attempt.kind, api::RemoteAttemptKind::Login));
        if self.remote.login.busy()
            || native_attempt.is_some() && self.remote.login.phase != login::Phase::Error
        {
            let cancel = native_attempt.map_or(Operation::CancelLogin, |attempt| {
                Operation::CancelAttempt(attempt.id.clone())
            });
            actions = actions
                .child(progress::spinner(
                    self.remote
                        .login_started
                        .unwrap_or_else(std::time::Instant::now),
                    p,
                ))
                .child(self.button(
                    "cancel-tailscale-login",
                    "Cancel sign-in",
                    Action::Remote(cancel),
                    cx,
                ));
        } else {
            actions = actions.child(
                self.button(
                    "tailscale-sign-in",
                    if self.remote.login.phase == login::Phase::Error
                        || state.status == api::NetworkStatus::Error
                    {
                        "Retry sign-in"
                    } else {
                        "Sign in to Tailscale"
                    },
                    Action::Remote(Operation::Login),
                    cx,
                )
                .border_color(p.accent),
            );
        }
        if self.remote.login.url.is_some()
            || (state.status == api::NetworkStatus::LoginRequired && state.login_url.is_some())
        {
            actions = actions.child(self.button(
                "continue-tailscale-login",
                "Continue in browser",
                Action::Remote(Operation::OpenLogin),
                cx,
            ));
        }
        account = account.child(actions);
        if self.remote.login.phase == login::Phase::Error {
            account = account.child(
                controls::hint(self.remote.login.message.clone(), p)
                    .mt(px(4.))
                    .text_size(px(13.))
                    .text_color(gpui::rgb(0xfc8397)),
            );
            if let Some(url) = &self.remote.login.url {
                if self.remote.login_address.read(cx).text != *url {
                    self.remote
                        .login_address
                        .update(cx, |field, cx| field.set_text(url.clone(), cx));
                }
                let address = self.bind_field(
                    self.remote.login_address.clone(),
                    "Tailscale login address",
                    None,
                    cx,
                );
                account = account.child(div().mt(px(8.)).child(address));
            }
        }
        let disclosure = self
            .bare_button(
                "advanced-sign-in",
                "Advanced sign-in",
                Action::Remote(Operation::Advanced),
                cx,
            )
            .expanded(self.remote.advanced)
            .h(px(15.))
            .min_h(px(15.))
            .line_height(px(15.))
            .text_size(px(11.))
            .text_color(p.muted)
            .bg(gpui::transparent_black())
            .border_0()
            .py_0()
            .pl(px(11.))
            .pr_0()
            .child(disclosure_marker(self.remote.advanced, p.muted));
        account = account.child(div().mt(px(12.)).flex().child(disclosure));
        if self.remote.advanced {
            let auth = self.bind_field(self.remote.auth.clone(), "Tailscale auth key", None, cx);
            let disabled =
                self.remote.login.busy() || self.remote.auth.read(cx).text.trim().is_empty();
            let use_key = self
                .button(
                    "use-tailscale-key",
                    "Use key",
                    Action::Remote(Operation::UseKey),
                    cx,
                )
                .when(disabled, |style| style.opacity(0.4));
            account = account
                .child(
                    div()
                        .mt(px(10.))
                        .mb(px(6.))
                        .max_w(px(440.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .on_key_down(cx.listener(
                            |panel, event: &gpui::KeyDownEvent, window, cx| {
                                if event.keystroke.key == "enter"
                                    && !panel.remote.auth.read(cx).is_composing()
                                {
                                    panel.remote_action(Operation::UseKey, cx);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .child(
                            div()
                                .w(px(55.))
                                .text_size(px(11.))
                                .text_color(p.muted)
                                .child("Auth key"),
                        )
                        .child(div().flex_1().min_w(px(100.)).child(auth))
                        .child(use_key),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .text_color(p.muted)
                        .child("Used once. Not saved by Shellfox."),
                );
        }
        account
    }
}
