use super::remote::{ErrorSection, Operation};
use super::{
    controls::{self, Action},
    *,
};
pub(super) fn connection_badge(
    label: impl Into<gpui::SharedString>,
    color: gpui::Hsla,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .px(px(6.))
        .py(px(2.))
        .border_1()
        .rounded(px(3.))
        .text_size(px(10.))
        .line_height(px(14.))
        .text_color(color)
        .child(div().size(px(5.)).rounded_full().bg(color))
        .child(label.into())
}

impl SettingsPanel {
    pub(super) fn remote_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        if self.services.is_none() || fixture::value("REMOTE").as_deref() == Some("unsupported") {
            return div().into_any_element();
        }
        let state = self.remote.state.clone();
        let toggle = self.toggle(
            "allow-remote-access".into(),
            state.as_ref().is_some_and(|state| state.enabled),
            Action::Remote(Operation::ToggleEnabled),
            cx,
        );
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
            ));
        let Some(state) = state else {
            return access
                .child(controls::hint(
                    self.remote
                        .read_error
                        .clone()
                        .unwrap_or_else(|| "Reading connection status…".into()),
                    p,
                ))
                .into_any_element();
        };
        let save = self
            .button(
                "save-machine-name",
                "Save",
                Action::Remote(Operation::SaveHostname),
                cx,
            )
            .h(px(26.))
            .line_height(px(16.));
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
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_size(px(12.))
                            .child("Tailscale"),
                    )
                    .child(connection_badge(status, badge_color).border_color(p.border)),
            )
            .child(
                controls::hint(
                    if online {
                        state
                            .error
                            .clone()
                            .filter(|error| !error.is_empty())
                            .unwrap_or_else(|| "This device is connected to your tailnet.".into())
                    } else if !state.enabled {
                        "Enable remote access to connect this device.".into()
                    } else if !self.remote.login.message.is_empty() {
                        self.remote.login.message.clone()
                    } else {
                        state
                            .error
                            .clone()
                            .filter(|error| !error.is_empty())
                            .unwrap_or_else(|| "Sign in to find your other machines.".into())
                    },
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
            .mt(px(14.))
            .mb(px(14.))
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
        access = access.child(identity).child(account);
        if let Some(error) = self.remote.error_at(ErrorSection::Access) {
            access = access.child(controls::error(error));
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
