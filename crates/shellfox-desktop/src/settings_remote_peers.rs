use super::remote::{Operation, connection};
use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn remote_machines(
        &mut self,
        state: &api::RemoteState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let remembered = state.peers.iter().filter(|peer| peer.remembered).count();
        let mut machines = controls::section(p)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(8.))
                    .child(controls::heading("Machines").mb_0())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(format!("{remembered} saved")),
                    ),
            )
            .child(controls::hint(
                "Connect to Shellfox on another device. Sessions run there, not here.",
                p,
            ));
        let mut saved_list = div()
            .id("saved-machines")
            .max_h(px(180.))
            .overflow_y_scroll();
        if remembered == 0 {
            saved_list = saved_list.child(
                div()
                    .py(px(10.))
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("No saved machines"),
            );
        }
        for (index, peer) in state
            .peers
            .iter()
            .filter(|peer| peer.remembered)
            .enumerate()
        {
            saved_list = saved_list.child(self.peer_row(peer, true, index + 1 == remembered, cx));
        }
        machines = machines.child(saved_list).child(
            div()
                .mt(px(16.))
                .mb(px(6.))
                .text_size(px(10.))
                .text_color(p.muted)
                .child("AVAILABLE ON YOUR TAILNET"),
        );
        let available: Vec<_> = state
            .peers
            .iter()
            .filter(|peer| {
                !peer.remembered
                    && (peer.host_confirmed || peer.connection != api::Connection::Disconnected)
            })
            .collect();
        let mut available_list = div()
            .id("available-machines")
            .max_h(px(180.))
            .overflow_y_scroll();
        if available.is_empty() {
            available_list = available_list.child(
                div()
                    .py(px(10.))
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("No other machines found"),
            );
        }
        let available_count = available.len();
        for (index, peer) in available.into_iter().enumerate() {
            available_list =
                available_list.child(self.peer_row(peer, false, index + 1 == available_count, cx));
        }
        machines.child(available_list).into_any_element()
    }
    pub(super) fn remote_clients(
        &mut self,
        state: &api::RemoteState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let mut clients = controls::section(p)
            .child(controls::heading("Client access"))
            .child(controls::hint(
                "Approve only devices you recognize. Revoking access leaves sessions running.",
                p,
            ));
        if !state.pending.is_empty() {
            clients = clients.child(
                div()
                    .mt(px(16.))
                    .mb(px(6.))
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child("WAITING FOR APPROVAL"),
            );
        }
        for peer in &state.pending {
            let approve = self.button(
                format!("approve-{}", peer.id),
                "Approve",
                Action::Remote(Operation::Approve(peer.id.clone())),
                cx,
            );
            let reject = self
                .text_button(
                    format!("reject-{}", peer.id),
                    "",
                    Action::Remote(Operation::Reject(peer.id.clone())),
                    cx,
                )
                .child(crate::assets::icon("close"));
            clients = clients.child(
                div()
                    .py(px(7.))
                    .min_h(px(42.))
                    .border_b_1()
                    .border_color(p.border)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .child(peer.hostname.clone())
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(format!("Device ID: {}", peer.id)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(peer.addresses.as_deref().unwrap_or(&[]).join(", ")),
                            ),
                    )
                    .child(approve)
                    .child(reject),
            );
        }
        for approval in &state.approvals {
            let mut row = div()
                .py(px(7.))
                .min_h(px(42.))
                .border_b_1()
                .border_color(p.border)
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div().flex_1().child(approval.hostname.clone()).child(
                        div()
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child(format!("Device ID: {}", approval.peer_id)),
                    ),
                );
            if self.remote.revoking.as_deref() == Some(&approval.peer_id) {
                row = row
                    .child("Revoke access?")
                    .child(self.button(
                        format!("confirm-revoke-{}", approval.peer_id),
                        "Revoke",
                        Action::Remote(Operation::ConfirmRevoke(approval.peer_id.clone())),
                        cx,
                    ))
                    .child(self.text_button(
                        format!("cancel-revoke-{}", approval.peer_id),
                        "Cancel",
                        Action::Remote(Operation::CancelRevoke),
                        cx,
                    ));
            } else {
                row = row.child(self.text_button(
                    format!("revoke-{}", approval.peer_id),
                    "Revoke",
                    Action::Remote(Operation::Revoke(approval.peer_id.clone())),
                    cx,
                ));
            }
            clients = clients.child(row);
        }
        if state.pending.is_empty() && state.approvals.is_empty() {
            clients = clients.child(
                div()
                    .py(px(10.))
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("No clients have access"),
            );
        }
        clients.into_any_element()
    }
    fn peer_row(
        &mut self,
        peer: &api::Peer,
        saved: bool,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let mut status = if saved || peer.connection != api::Connection::Disconnected {
            connection(peer).to_owned()
        } else {
            peer.addresses.join(", ")
        };
        if saved && let Some(error) = &peer.last_error {
            status.push_str(" · ");
            status.push_str(error);
        }
        let mut row = div()
            .py(px(7.))
            .min_h(px(42.))
            .flex()
            .items_center()
            .gap(px(8.))
            .when(!last, |row| row.border_b_1().border_color(p.border))
            .child(
                div().flex_1().child(peer.hostname.clone()).child(
                    div()
                        .mt(px(2.))
                        .text_size(px(10.))
                        .text_color(p.muted)
                        .child(status),
                ),
            );
        let attempt=self.remote.state.as_ref().and_then(|state|state.attempts.iter().find(|attempt|matches!(&attempt.kind,api::RemoteAttemptKind::Connect{peer_id}if peer_id==&peer.id))).map(|attempt|attempt.id.clone());
        if let Some(attempt_id) = attempt {
            row = row.child(self.button(
                format!("cancel-peer-{}", peer.id),
                "Cancel request",
                Action::Remote(Operation::CancelAttempt(attempt_id)),
                cx,
            ));
        } else if peer.connection == api::Connection::AwaitingApproval {
            row = row.child(self.button(
                format!("cancel-peer-{}", peer.id),
                "Cancel request",
                Action::Remote(Operation::Disconnect(peer.id.clone())),
                cx,
            ));
        } else if peer.connection == api::Connection::Rejected {
            row = row.child(self.button(
                format!("request-again-{}", peer.id),
                "Request again",
                Action::Remote(Operation::Connect(peer.id.clone())),
                cx,
            ));
        } else if !saved {
            row = row.child(
                self.button(
                    format!("connect-peer-{}", peer.id),
                    if peer.connection == api::Connection::Disconnected {
                        "Connect"
                    } else {
                        connection(peer)
                    },
                    Action::Remote(Operation::Connect(peer.id.clone())),
                    cx,
                )
                .when(
                    !peer.online || peer.connection != api::Connection::Disconnected,
                    |style| style.opacity(0.4),
                ),
            );
        }
        if saved {
            row = row.child(
                self.text_button(
                    format!("forget-peer-{}", peer.id),
                    "",
                    Action::Remote(Operation::Forget(peer.id.clone())),
                    cx,
                )
                .child(crate::assets::icon("close")),
            );
        }
        row.into_any_element()
    }
}
