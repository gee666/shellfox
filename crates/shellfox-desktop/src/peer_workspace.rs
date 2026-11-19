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
    #[cfg(feature = "experimental-shellfox-native")]
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
    #[cfg(not(feature = "experimental-shellfox-native"))]
    pub(crate) fn ensure_peer_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_v1_terminal(window, cx);
    }

    pub(crate) fn peer_workspace(
        &self,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((peer, state)) = self.peers.selected_peer() else {
            return div().into_any_element();
        };
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        if state.phase != source::Phase::Connected {
            return self.peer_connection_panel(peer, state, palette, cx);
        }
        let mut pane = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        let files_available = self.peer_api(&cycle).is_ok();
        let session = state.snapshot.as_ref().and_then(|snapshot| {
            source::sessions(snapshot).find(|session| Some(session.id) == state.selected_session)
        });
        let visible_selected = state.selected_tab.filter(|id| session.is_some_and(|session| {
            session.tabs.iter().any(|tab| tab.id == *id && source::viewable_tab(tab))
        }));
        if let Some(session) = session {
            let mut tabs = div()
                .flex()
                .h(px(30.))
                .gap(px(3.))
                .px(px(4.))
                .border_b_1()
                .border_color(palette.border);
            for tab in session.tabs.iter().filter(|tab| source::viewable_tab(tab)) {
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
            pane = pane.child(tabs.when(files_available, |tabs| tabs.child(div().flex_1()).child(self.render_files_toggle(true, palette, cx))));
        } else if files_available {
            pane = pane.child(div().h(px(30.)).flex().justify_end().border_b_1().border_color(palette.border)
                .child(self.render_files_toggle(true, palette, cx)));
        }
        #[cfg(feature = "experimental-shellfox-native")]
        if let Some(tab) = visible_selected
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
        #[cfg(not(feature = "experimental-shellfox-native"))]
        if let Some(view) = visible_selected.and_then(|tab| self.peer_ui.v1_views.get(&(peer.clone(), tab))) {
            pane = pane.child(view.clone());
        }
        self.with_peer_files(pane, palette, window, cx)
    }
}
