use super::*;
use crate::{
    assets::{icon_button, tooltip},
    chrome::button,
};
use anyhow::{Result, anyhow, ensure};
use metadata::PeerApi;
impl Root {
    pub(crate) fn selected_peer_api(&self) -> Result<(source::PeerCycle, PeerApi)> {
        ensure!(
            !self.peer_ui.frozen,
            "Peer operations are frozen for window close."
        );
        let (peer, state) = self
            .peers
            .selected_peer()
            .ok_or_else(|| anyhow!("No peer source is selected."))?;
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        let api = self.peer_api(&cycle)?;
        Ok((cycle, api))
    }
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
            return self.with_peer_files(div(), palette, window, cx);
        };
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        if state.phase != source::Phase::Connected {
            let connection = self.peer_connection_panel(peer, state, palette, cx);
            return self.with_peer_files(connection, palette, window, cx);
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
            pane = pane.child(tabs.when(files_available, |tabs| {
                tabs.child(div().flex_1())
                    .child(self.render_files_toggle(true, palette, cx))
                    .when(visible_selected.is_some(), |tabs| {
                        tabs.child(self.workspace_button("workspace/peer/terminal-folder", "Open terminal folder",
                            icon_button("peer-terminal-folder", "folder-terminal", palette)
                                .tab_index(0)
                                .tooltip(tooltip("Open terminal folder", palette))
                                .on_click(cx.listener(|root, _, _, cx| root.open_peer_terminal_folder(cx))),
                        ))
                    })
            }));
        } else if files_available {
            pane = pane.child(div().h(px(30.)).flex().justify_end().border_b_1().border_color(palette.border)
                .child(self.render_files_toggle(true, palette, cx)));
        }
        if let Some(view) = visible_selected.and_then(|tab| self.peer_ui.v1_views.get(&(peer.clone(), tab))) {
            pane = pane.child(view.clone());
        }
        self.with_peer_files(pane, palette, window, cx)
    }
}
