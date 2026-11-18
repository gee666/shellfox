use super::*;
use crate::{
    chrome::button,
    host_client::peer::{Connection, SourceAdmission},
};
use anyhow::{Result, anyhow, ensure};
use std::sync::Arc;
impl Root {
    pub(crate) fn peer_context(
        &self,
    ) -> Result<(source::PeerCycle, Arc<Connection>, SourceAdmission)> {
        ensure!(
            !self.peer_ui.frozen,
            "Peer operations are frozen for window close."
        );
        let (peer, state) = self
            .peers
            .selected_peer()
            .ok_or_else(|| anyhow!("No peer source is selected."))?;
        ensure!(
            state.phase == source::Phase::Connected,
            "Peer is not connected."
        );
        let witness = state
            .origin
            .as_ref()
            .ok_or_else(|| anyhow!("Peer catalog origin is unavailable."))?;
        let connection = self
            .host
            .as_ref()
            .ok_or_else(|| anyhow!("Host is unavailable."))?
            .peer_existing(peer)?;
        let admission = connection.admit_source_with(witness)?;
        Ok((
            source::PeerCycle {
                peer: peer.clone(),
                generation: state.cycle,
            },
            connection,
            admission,
        ))
    }
    pub(crate) fn focus_peer_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((peer, _)) = self.peers.selected_peer()
            && let Some((path, _)) = self.peer_ui.forms.get(peer)
        {
            path.read(cx).focus(window);
            cx.notify();
        }
    }
    pub(crate) fn ensure_peer_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((peer, state)) = self.peers.selected_peer() else {
            return;
        };
        let Some(tab) = state.selected_tab else {
            return;
        };
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        self.open_peer_terminal(cycle, tab, window, cx);
    }
    pub(crate) fn peer_workspace(
        &self,
        palette: Palette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((peer, state)) = self.peers.selected_peer() else {
            return div().into_any_element();
        };
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        let mut pane = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        let reconnect = peer.clone();
        let disconnect = peer.clone();
        let mut toolbar = div()
            .flex()
            .items_center()
            .h(px(32.))
            .gap(px(6.))
            .px(px(8.))
            .border_b_1()
            .border_color(palette.border)
            .child(format!("{} · {:?}", state.hostname, state.phase))
            .child(
                button("peer-reconnect", "Reconnect", palette).on_click(cx.listener(
                    move |root, _, window, cx| {
                        root.connect_peer(reconnect.clone(), true, window, cx)
                    },
                )),
            )
            .child(
                button("peer-disconnect", "Disconnect", palette).on_click(cx.listener(
                    move |root, _, window, cx| root.disconnect_peer(disconnect.clone(), window, cx),
                )),
            );
        if let Some(error) = &state.error {
            toolbar = toolbar.child(div().text_color(palette.accent).child(error.clone()));
        }
        pane = pane.child(toolbar);
        if state.phase != source::Phase::Connected {
            return pane.into_any_element();
        }
        if let Some((path, title)) = self.peer_ui.forms.get(peer) {
            pane = pane.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .p(px(6.))
                    .child(div().w(px(260.)).child(path.clone()))
                    .child(div().w(px(150.)).child(title.clone()))
                    .child(button("peer-create", "New session", palette).on_click(
                        cx.listener(|root, _, window, cx| root.create_peer_session(window, cx)),
                    )),
            );
        }
        let session = state.snapshot.as_ref().and_then(|snapshot| {
            source::sessions(snapshot).find(|session| Some(session.id) == state.selected_session)
        });
        if let Some(session) = session {
            let mut tabs = div()
                .flex()
                .h(px(30.))
                .gap(px(3.))
                .px(px(4.))
                .border_b_1()
                .border_color(palette.border);
            for tab in &session.tabs {
                let id = tab.id;
                let session = session.id;
                let target = cycle.clone();
                tabs = tabs.child(
                    button(
                        gpui::SharedString::from(format!("peer-tab-{id}")),
                        tab.title.clone(),
                        palette,
                    )
                    .on_click(cx.listener(move |root, _, window, cx| {
                        root.select_peer_session(target.clone(), session, Some(id), window, cx)
                    })),
                );
            }
            tabs = tabs.child(
                button("peer-add-tab", "+", palette)
                    .on_click(cx.listener(|root, _, window, cx| root.add_peer_tab(window, cx))),
            );
            pane = pane.child(tabs);
        }
        if let Some(tab) = state.selected_tab
            && let Some(view) = self.peer_ui.views.get(&(peer.clone(), tab))
        {
            let terminal = view.read(cx);
            let status = terminal.error.clone().unwrap_or_else(|| {
                if terminal.ready {
                    format!("{:?}", terminal.input_state)
                } else {
                    "Recovering native state…".into()
                }
            });
            pane = pane
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(24.))
                        .px(px(8.))
                        .gap(px(8.))
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child(status)
                        .child(
                            button("peer-take-control", "Take control", palette).on_click(
                                cx.listener(|root, _, window, cx| {
                                    root.take_peer_control(window, cx)
                                }),
                            ),
                        ),
                )
                .child(view.clone());
        }
        pane.into_any_element()
    }
}
