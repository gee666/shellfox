use super::remote::Operation;
use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn remote_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let Some(state) = self.remote.state.clone() else {
            return div()
                .child(controls::heading("Remote access"))
                .child(controls::hint(
                    self.remote.error.clone().unwrap_or_else(|| {
                        if self.services.is_some() {
                            "Reading connection status…"
                        } else {
                            "Remote access is unavailable on the connected host."
                        }
                        .into()
                    }),
                    p,
                ))
                .into_any_element();
        };
        let toggle = self
            .toggle(
                "allow-remote-access".into(),
                state.enabled,
                Action::Remote(Operation::ToggleEnabled),
                cx,
            )
            .when(self.remote.pending || self.remote.login.busy(), |style| {
                style.opacity(0.4)
            });
        let save = self
            .button(
                "save-machine-name",
                "Save",
                Action::Remote(Operation::SaveHostname),
                cx,
            )
            .when(
                self.remote.pending
                    || self.remote.hostname.read(cx).text.trim().is_empty()
                    || self.remote.hostname.read(cx).text == state.hostname,
                |style| style.opacity(0.4),
            );
        let online = state.status == api::NetworkStatus::Online;
        let status = if !state.enabled {
            "Disabled"
        } else if self.remote.login.busy() {
            "Signing in"
        } else {
            match state.status {
                api::NetworkStatus::Online => "Connected",
                api::NetworkStatus::Starting => "Starting",
                api::NetworkStatus::Error => "Connection error",
                _ => "Not connected",
            }
        };
        let badge_color: gpui::Hsla = if online {
            gpui::rgb(0x4ade80).into()
        } else {
            p.muted
        };
        let mut account = div()
            .px(px(14.))
            .py(px(12.))
            .border_1()
            .border_color(p.border)
            .rounded(px(3.))
            .bg(p.sidebar)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .mb(px(5.))
                    .child("Tailscale")
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .px(px(6.))
                            .py(px(2.))
                            .border_1()
                            .border_color(p.border)
                            .rounded(px(3.))
                            .text_size(px(10.))
                            .text_color(badge_color)
                            .child(div().size(px(5.)).rounded_full().bg(badge_color))
                            .child(status),
                    ),
            )
            .child(
                controls::hint(
                    (if !online && state.enabled && !self.remote.login.message.is_empty() {
                        Some(self.remote.login.message.clone())
                    } else {
                        state.error.clone()
                    })
                    .unwrap_or_else(|| {
                        if online {
                            "This device is connected to your tailnet."
                        } else if !state.enabled {
                            "Enable remote access to connect this device."
                        } else {
                            "Sign in to find your other machines."
                        }
                        .into()
                    }),
                    p,
                )
                .mb_0(),
            );
        if state.enabled && !online {
            account = self.remote_login_controls(account, &state, cx);
        }
        let hostname = self.bind_field(
            self.remote.hostname.clone(),
            "Remote machine name",
            None,
            cx,
        );
        let identity = div()
            .on_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter"
                        && !panel.remote.hostname.read(cx).is_composing()
                    {
                        panel.remote_action(Operation::SaveHostname, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }),
            )
            .my(px(14.))
            .max_w(px(440.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .w(px(90.))
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("Machine name"),
            )
            .child(div().flex_1().min_w(px(100.)).child(hostname))
            .child(save);
        let mut access = div()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .mb(px(8.))
                    .child(controls::heading("Remote access").mb_0())
                    .child(toggle),
            )
            .child(controls::hint(
                "Let approved clients use sessions on this device.",
                p,
            ))
            .child(identity)
            .child(account);
        if let Some(error) = &self.remote.error {
            access = access.child(
                controls::hint(error.clone(), p)
                    .mt(px(4.))
                    .text_color(gpui::rgb(0xfc8397)),
            );
        }
        for limitation in &state.limitations {
            access = access.child(controls::hint(limitation.clone(), p).mt(px(4.)));
        }
        let machines = self.remote_machines(&state, cx);
        let clients = self.remote_clients(&state, cx);
        div()
            .child(access)
            .child(machines)
            .child(clients)
            .into_any_element()
    }
}
