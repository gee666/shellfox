use super::*;
use crate::{assets::icon_sized, dialogs::dialog_button};

pub(super) const CONNECTION_FAILURE: &str =
    "Remote connection failed. Reconnect before retrying an uncertain operation.";

impl Root {
    pub(super) fn peer_connection_panel(
        &self,
        peer: &AuthenticatedPeerId,
        state: &source::PeerState,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        use source::Phase;
        let waiting = state.phase == Phase::AwaitingApproval;
        let busy = matches!(
            state.phase,
            Phase::AwaitingApproval | Phase::Connecting | Phase::Reconnecting
        );
        let title = match state.phase {
            Phase::AwaitingApproval => "Waiting for approval".into(),
            Phase::Connecting | Phase::Reconnecting => format!("Connecting to {}", state.hostname),
            Phase::Rejected => "Request declined".into(),
            _ if state.online => "Machine disconnected".into(),
            _ => "Machine offline".into(),
        };
        let detail = if waiting {
            format!("Approve this machine in Shellfox on {}.", state.hostname)
        } else {
            state.error.clone().unwrap_or_else(|| {
                "Its terminals are owned by that machine. Reconnect to check their state.".into()
            })
        };
        let target = peer.clone();
        let action = if busy {
            "Cancel request"
        } else if state.phase == Phase::Rejected {
            "Request access again"
        } else {
            "Connect"
        };
        let enabled = busy || state.online;
        let panel = div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .items_center()
            .justify_center()
            .flex_col()
            .gap(px(10.))
            .p(px(32.))
            .text_color(palette.muted)
            .text_center()
            .child(
                div()
                    .size(px(36.))
                    .mb(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .rounded(px(5.))
                    .border_color(if busy {
                        palette.accent.opacity(0.4)
                    } else {
                        palette.border
                    })
                    .text_color(if busy { palette.accent } else { palette.muted })
                    .child(icon_sized(if waiting { "shield" } else { "server" }, 19.)),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .line_height(px(20.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(palette.foreground)
                    .child(title),
            )
            .child(
                div()
                    .max_w(px(340.))
                    .text_size(px(12.))
                    .line_height(px(19.2))
                    .child(detail),
            )
            .when(
                busy && self.peer_ui.pending_create.as_ref() == Some(peer),
                |panel| {
                    panel.child(
                        div()
                            .max_w(px(340.))
                            .text_size(px(11.))
                            .child("Your folder prompt will open once the connection is ready."),
                    )
                },
            )
            .child(
                dialog_button("peer-connect-action", action, palette)
                    .mt(px(4.))
                    .px(px(12.))
                    .h(px(29.))
                    .text_size(px(13.))
                    .line_height(px(17.))
                    .when(!enabled, |button| button.opacity(0.4))
                    .when(enabled, |button| {
                        button.on_click(cx.listener(move |root, _, window, cx| {
                            if busy {
                                root.disconnect_peer(target.clone(), window, cx);
                            } else {
                                root.connect_peer(target.clone(), false, window, cx);
                            }
                        }))
                    }),
            );
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .flex()
                    .justify_end()
                    .items_center()
                    .bg(palette.sidebar)
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.4)
                            .child(icon_sized("folder", 16.).text_color(palette.muted)),
                    ),
            )
            .child(self.semantic_region(
                "workspace/peer-connection",
                crate::accessibility::Role::Status,
                "Peer connection",
                panel,
            ))
            .into_any_element()
    }
}
