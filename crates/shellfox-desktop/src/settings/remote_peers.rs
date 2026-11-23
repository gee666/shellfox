use super::remote::{ErrorSection, Operation};
use super::remote_page::connection_badge;
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
                            .line_height(px(15.))
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
                    .line_height(px(15.))
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
            saved_list = saved_list.child(self.peer_row(peer, index + 1 == remembered, cx));
        }
        machines = machines.child(saved_list).child(
            div()
                .mt(px(16.))
                .mb(px(6.))
                .text_size(px(10.))
                .line_height(px(14.))
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
                    .line_height(px(15.))
                    .text_color(p.muted)
                    .child("No other machines found"),
            );
        }
        let available_count = available.len();
        for (index, peer) in available.into_iter().enumerate() {
            available_list =
                available_list.child(self.peer_row(peer, index + 1 == available_count, cx));
        }
        machines
            .child(available_list)
            .when_some(
                self.remote.error_at(ErrorSection::Machines),
                |section, error| section.child(controls::error(error)),
            )
            .into_any_element()
    }
    pub(super) fn remote_clients(
        &mut self,
        state: &api::RemoteState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let pending = self.access_review.clone();
        let focus = self.access_focus.clone();
        let scroll = self.body_scroll.clone();
        let mut clients = controls::section(p)
            .relative()
            .track_focus(&self.access_focus)
            .child(
                gpui::canvas(
                    move |section, window, cx| {
                        let viewport = scroll.bounds();
                        if viewport.size.height <= px(0.) || !pending.replace(false) {
                            return;
                        }
                        let delta = section.center().y - viewport.center().y;
                        let offset = scroll.offset();
                        scroll.set_offset(gpui::point(
                            offset.x,
                            (offset.y - delta).clamp(-scroll.max_offset().height, px(0.)),
                        ));
                        let focus = focus.clone();
                        window.defer(cx, move |window, _| {
                            focus.focus(window);
                            window.refresh();
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .mb(px(8.))
                    .child(controls::heading("Client access").mb_0())
                    .when(!state.pending.is_empty(), |heading| {
                        heading.child(
                            connection_badge(format!("{} pending", state.pending.len()), p.muted)
                                .border_color(p.border),
                        )
                    }),
            )
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
                            .when_some(
                                peer.addresses
                                    .as_ref()
                                    .filter(|addresses| !addresses.is_empty()),
                                |copy, addresses| {
                                    copy.child(
                                        div()
                                            .mt(px(2.))
                                            .text_size(px(10.))
                                            .text_color(p.muted)
                                            .child(addresses.join(", ")),
                                    )
                                },
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
        clients
            .when_some(
                self.remote.error_at(ErrorSection::Clients),
                |section, error| section.child(controls::error(error)),
            )
            .into_any_element()
    }
}
