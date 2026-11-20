use super::{api::Connection, controls::Action, remote::Operation, *};

pub(super) struct PeerRow {
    pub status: String,
    pub action: Option<(String, &'static str, Operation)>,
    pub forget: bool,
}

impl PeerRow {
    pub(super) fn new(
        connecting: bool,
        peer: &api::Peer,
        attempt: Option<&str>,
        pairing: &pairing::Scope,
    ) -> Self {
        if connecting {
            return Self {
                status: "Connecting…".into(),
                action: None,
                forget: false,
            };
        }
        let pending = match peer.connection {
            Connection::Connecting | Connection::AwaitingApproval => true,
            Connection::Disconnected | Connection::Reconnecting => attempt.is_some(),
            Connection::Connected | Connection::Rejected => false,
        };
        let status = if pending && attempt.is_some_and(|id| pairing.stopped(id)) {
            "Automatic checking stopped. Cancel the request and try again.".into()
        } else {
            let mut status = if peer.remembered || peer.connection != Connection::Disconnected {
                remote::connection(peer).to_owned()
            } else {
                peer.addresses.join(", ")
            };
            if let Some(error) = peer.last_error.as_ref().filter(|_| peer.remembered) {
                status.push_str(" · ");
                status.push_str(error);
            }
            status
        };
        let action = if pending {
            // Include the UUID in the control key too, so an old accessibility
            // action can't resolve to the replacement request's button.
            attempt.map(|id| {
                (
                    format!("cancel-peer-{id}"),
                    "Cancel request",
                    Operation::CancelPair(id.to_owned()),
                )
            })
        } else {
            match peer.connection {
                Connection::Connected => Some((
                    format!("disconnect-peer-{}", peer.id),
                    "Disconnect",
                    Operation::Disconnect(peer.id.clone()),
                )),
                Connection::Disconnected | Connection::Rejected => {
                    let (key, label) = if peer.remembered || peer.connection == Connection::Rejected
                    {
                        ("request-again", "Request again")
                    } else {
                        ("connect-peer", "Connect")
                    };
                    Some((
                        format!("{key}-{}", peer.id),
                        label,
                        Operation::Connect(peer.id.clone()),
                    ))
                }
                _ => None,
            }
        };
        Self {
            status,
            action,
            forget: peer.remembered && !pending,
        }
    }
}

impl SettingsPanel {
    pub(super) fn peer_row(
        &mut self,
        peer: &api::Peer,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let connecting = self.remote.connecting_peer.as_ref() == Some(&peer.id);
        let attempt = self.remote.state.as_ref().and_then(|state| {
            state
                .attempts
                .iter()
                .find_map(|attempt| match &attempt.kind {
                    api::RemoteAttemptKind::Connect { peer_id } if peer_id == &peer.id => {
                        Some(attempt.id.as_str())
                    }
                    _ => None,
                })
        });
        let view = PeerRow::new(connecting, peer, attempt, &self.remote.pairing_scope);
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
                        .child(view.status),
                ),
            );
        if let Some((id, label, action)) = view.action {
            row = row.child(self.button(id, label, Action::Remote(action), cx));
        }
        if view.forget {
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
