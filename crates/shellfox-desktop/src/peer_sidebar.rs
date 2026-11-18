use super::*;
use gpui::{Div,Stateful};
impl Root {
    pub(crate) fn peer_sidebar(
        &self,
        mut list: Stateful<Div>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut peers: Vec<_> = self.peers.peers.iter().collect();
        peers.sort_by(|a, b| a.1.hostname.cmp(&b.1.hostname));
        for (peer, state) in peers {
            let target = peer.clone();
            let selected = self.peers.selected == source::Source::Peer(peer.clone());
            list = list.child(
                div()
                    .id(gpui::SharedString::from(format!("peer-{}", peer.as_str())))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .px(px(9.))
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .cursor_pointer()
                    .when(selected, |style| style.bg(palette.row_hover))
                    .child(format!("{} · {:?}", state.hostname, state.phase))
                    .on_click(cx.listener(move |root, _, window, cx| {
                        root.choose_peer(target.clone(), window, cx)
                    })),
            );
            if !selected {
                continue;
            }
            let Some(snapshot) = &state.snapshot else {
                continue;
            };
            let shown = self
                .peer_ui
                .pages
                .get(peer)
                .copied()
                .unwrap_or(1)
                .saturating_mul(100);
            let cycle = source::PeerCycle {
                peer: peer.clone(),
                generation: state.cycle,
            };
            for entry in snapshot.entries.iter().take(shown) {
                match entry {
                    m::CatalogEntry::Session { value } => {
                        let target = cycle.clone();
                        let id = value.id;
                        let selected = state.selected_session == Some(id);
                        let cwd = value
                            .facts
                            .cwd
                            .known()
                            .cloned()
                            .unwrap_or_else(|| "Path unavailable".into());
                        list = list.child(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "peer-{}-session-{id}",
                                    peer.as_str()
                                )))
                                .px(px(14.))
                                .py(px(5.))
                                .flex()
                                .flex_col()
                                .cursor_pointer()
                                .when(selected, |style| style.bg(palette.row_hover))
                                .child(value.title.clone())
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(palette.muted)
                                        .child(cwd),
                                )
                                .on_click(cx.listener(move |root, _, window, cx| {
                                    root.select_peer_session(target.clone(), id, None, window, cx)
                                })),
                        );
                    }
                    m::CatalogEntry::Unavailable { session_id, issues } => {
                        list = list.child(
                            div()
                                .px(px(14.))
                                .py(px(5.))
                                .text_color(palette.muted)
                                .text_size(px(10.))
                                .child(format!(
                                    "Unavailable · {session_id} · {} issues",
                                    issues.len()
                                )),
                        );
                    }
                }
            }
            if snapshot.entries.len() > shown {
                let peer = peer.clone();
                list = list.child(
                    div()
                        .id("peer-more")
                        .px(px(14.))
                        .py(px(5.))
                        .cursor_pointer()
                        .child("More…")
                        .on_click(cx.listener(move |root, _, _, cx| {
                            let page = root.peer_ui.pages.entry(peer.clone()).or_insert(1);
                            *page = page.saturating_add(1);
                            cx.notify();
                        })),
                );
            }
        }
        list
    }
}
