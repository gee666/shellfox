//! The sidebar is one GPUI `list`: the Local section, then each remembered
//! peer. Only entries near the viewport are rendered. Each row passes a focus
//! handle to the list, so a focused row stays rendered while scrolled away.
use super::*;
use crate::session_visibility::PollingTarget;
use gpui::{Bounds, FocusHandle, ListAlignment, ListState, Pixels};
use shellfox_protocol::{AuthenticatedPeerId, manager_v2 as m};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    LocalHeader,
    Local(SessionId),
    LocalMore,
    LocalEmpty,
    PeersGap,
    PeerHeader(AuthenticatedPeerId),
    PeerEmpty(AuthenticatedPeerId),
    Peer(AuthenticatedPeerId, m::Id),
    PeerMore(AuthenticatedPeerId),
    PeerEnd(AuthenticatedPeerId),
}

#[derive(Clone)]
pub(crate) struct SidebarList {
    state: ListState,
    entries: Rc<RefCell<Vec<Entry>>>,
    painted: Rc<Cell<bool>>,
    palette: Rc<Cell<Option<Palette>>>,
}

impl Default for SidebarList {
    fn default() -> Self {
        Self {
            state: ListState::new(0, ListAlignment::Top, px(200.)).measure_all(),
            entries: Default::default(),
            painted: Default::default(),
            palette: Default::default(),
        }
    }
}

impl SidebarList {
    pub(crate) fn state(&self) -> &ListState {
        &self.state
    }

    /// Replace the entries, splicing only the changed middle so scroll
    /// position and measured heights survive.
    fn sync(&self, next: Vec<Entry>, focus: impl Fn(&Entry) -> Option<FocusHandle>) {
        let mut entries = self.entries.borrow_mut();
        if *entries == next {
            return;
        }
        let prefix = entries.iter().zip(&next).take_while(|(a, b)| a == b).count();
        let suffix = entries[prefix..]
            .iter()
            .rev()
            .zip(next[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let changed = &next[prefix..next.len() - suffix];
        self.state.splice_focusable(prefix..entries.len() - suffix, changed.iter().map(focus));
        *entries = next;
    }

    fn entry(&self, index: usize) -> Option<Entry> {
        self.entries.borrow().get(index).cloned()
    }

    fn index(&self, entry: &Entry) -> Option<usize> {
        self.entries.borrow().iter().position(|current| current == entry)
    }

    pub(crate) fn reveal(&self, entry: &Entry) {
        if let Some(index) = self.index(entry) {
            self.state.scroll_to_reveal_item(index);
        }
    }

    pub(crate) fn bounds(&self, entry: &Entry) -> Option<Bounds<Pixels>> {
        self.state.bounds_for_item(self.index(entry)?)
    }

    pub(crate) fn begin_frame(&self) {
        self.painted.set(false);
    }

    /// Paints nothing; records that the list was on screen this frame.
    pub(crate) fn painted_marker(&self) -> impl IntoElement + use<> {
        let painted = self.painted.clone();
        gpui::canvas(|_, _, _| (), move |_, (), _, _| painted.set(true))
            .absolute()
            .inset_0()
    }

    /// Sessions whose rows are on screen in the last painted frame.
    pub(crate) fn visible_sessions(&self) -> Vec<PollingTarget> {
        if !self.painted.get() {
            return Vec::new();
        }
        let viewport = self.state.viewport_bounds();
        let entries = self.entries.borrow();
        let mut visible = Vec::new();
        for index in self.state.logical_scroll_top().item_ix..entries.len() {
            let Some(bounds) = self.state.bounds_for_item(index) else {
                break;
            };
            if bounds.top() >= viewport.bottom() {
                break;
            }
            if !crate::session_visibility::intersects(bounds, viewport) {
                continue;
            }
            match &entries[index] {
                Entry::Local(id) => visible.push(PollingTarget::Local(*id)),
                Entry::Peer(peer, id) => {
                    if let Ok(id) = id.to_string().parse() {
                        visible.push(PollingTarget::Peer(peer.clone(), id));
                    }
                }
                _ => {}
            }
        }
        visible
    }
}

impl Root {
    pub(super) fn sidebar_list(&self, palette: Palette, window: &Window, cx: &mut Context<Self>) -> gpui::List {
        let mut entries = Vec::new();
        if self.snapshot.is_some() {
            entries.push(Entry::LocalHeader);
            if self.local_expanded {
                let rows = self.sidebar_rows(cx);
                let shown = rows.len().min(self.session_pages.saturating_mul(self.history_page_size));
                entries.extend(rows[..shown].iter().map(|(session, _)| Entry::Local(session.id)));
                if shown < rows.len() {
                    entries.push(Entry::LocalMore);
                }
                if rows.is_empty() {
                    entries.push(Entry::LocalEmpty);
                }
            }
            if self.peers.peers.values().any(|peer| peer.remembered) {
                entries.push(Entry::PeersGap);
            }
        }
        self.peer_entries(&mut entries, window, cx);
        let list = &self.sidebar_ui.list;
        list.palette.set(Some(palette));
        list.sync(entries, |entry| self.entry_focus(entry));
        gpui::list(
            list.state.clone(),
            cx.processor(|root: &mut Root, index, window, cx| root.render_sidebar_entry(index, window, cx)),
        )
    }

    fn entry_focus(&self, entry: &Entry) -> Option<FocusHandle> {
        match entry {
            Entry::Local(id) => self.sidebar_ui.row_focus.get(id).map(|focus| focus.row.clone()),
            Entry::Peer(peer, id) => self.peer_row_focus(peer, *id),
            _ => None,
        }
    }

    fn render_sidebar_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let list = &self.sidebar_ui.list;
        let (Some(entry), Some(palette)) = (list.entry(index), list.palette.get()) else {
            return div().into_any_element();
        };
        match entry {
            Entry::LocalHeader => self.local_header(palette, cx),
            Entry::Local(id) => {
                let Some(session) = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
                else {
                    return div().into_any_element();
                };
                let dot = self.session_agent_state(session, cx);
                self.session_row(session, dot, palette, window, cx)
            }
            Entry::LocalMore => self.local_more(palette, cx),
            Entry::LocalEmpty => empty_sessions(palette).into_any_element(),
            Entry::PeersGap => div().h(px(4.)).into_any_element(),
            entry => self.render_peer_entry(&entry, palette, window, cx),
        }
    }

    fn local_header(&self, palette: Palette, cx: &Context<Self>) -> AnyElement {
        let count = self.snapshot.as_ref().map_or(0, |snapshot| snapshot.sessions.len());
        self.workspace_button(
            "workspace/local/sessions",
            "Local sessions",
            div()
                .id("local-section")
                .track_focus(&self.sidebar_ui.group)
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(9.))
                .pr(px(4.))
                .text_size(px(11.))
                .text_color(palette.muted)
                .cursor_pointer()
                .hover(|style| style.bg(palette.hover))
                .child(div().flex_shrink_0().child(icon_sized(
                    if self.local_expanded { "down" } else { "chevron" },
                    12.,
                )))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .relative()
                        .top(px(1.))
                        .child(format!("Local · {count}")),
                )
                .on_click(cx.listener(|root, _, window, cx| root.toggle_local_group(window, cx))),
        )
    }

    fn local_more(&self, palette: Palette, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .mx(px(12.))
            .my(px(4.))
            .child(
                div()
                    .id("more-sessions")
                    .track_focus(&self.sidebar_ui.more)
                    .p(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(palette.muted)
                    .cursor_pointer()
                    .hover(|style| style.text_color(palette.accent))
                    .child(div().relative().top(px(1.)).child("Show more"))
                    .on_click(cx.listener(|root, _, window, cx| root.show_more_sessions(window, cx))),
            )
            .into_any_element()
    }

    pub(super) fn sidebar_focus_tab(
        &self,
        id: SessionId,
        shift: bool,
        window: &mut Window,
        cx: &gpui::App,
    ) {
        use crate::root::Dialog;
        let Some(focus) = self.sidebar_ui.row_focus.get(&id) else {
            return;
        };
        let focus_copy = |target: SessionId, window: &mut Window| {
            if let Some(Dialog::RenameSession { id, title }) = &self.dialog
                && *id == target
            {
                title.read(cx).focus(window);
            } else {
                self.sidebar_ui.focus_row(target, window);
            }
        };
        let on_settle = focus.settle.is_focused(window);
        if on_settle && shift {
            focus_copy(id, window);
            return;
        }
        if !on_settle && !shift && !self.user_action_pending {
            focus.settle.focus(window);
            return;
        }
        let rows = self.sidebar_rows(cx);
        let shown = rows
            .len()
            .min(self.session_pages.saturating_mul(self.history_page_size));
        let Some(index) = rows[..shown]
            .iter()
            .position(|(session, _)| session.id == id)
        else {
            return;
        };
        let next = if shift {
            index.checked_sub(1)
        } else {
            (index + 1 < shown).then_some(index + 1)
        };
        if let Some(next) = next {
            let target = rows[next].0.id;
            if shift
                && !self.user_action_pending
                && let Some(focus) = self.sidebar_ui.row_focus.get(&target)
            {
                focus.settle.focus(window);
            } else {
                focus_copy(target, window);
            }
            self.sidebar_ui.list.reveal(&Entry::Local(target));
        } else if shift {
            self.sidebar_ui.group.focus(window);
        } else if shown < rows.len() {
            self.sidebar_ui.more.focus(window);
            self.sidebar_ui.list.reveal(&Entry::LocalMore);
        } else {
            window.focus_next();
        }
    }

    pub(crate) fn reveal_sidebar_session(&self, id: SessionId, _: &gpui::App) {
        if self.sidebar && self.local_expanded {
            self.sidebar_ui.list.reveal(&Entry::Local(id));
        }
    }
}

pub(crate) fn empty_sessions(palette: Palette) -> gpui::Div {
    div()
        .mx(px(8.))
        .my(px(12.))
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(palette.muted)
        .child("No sessions, press +")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &SidebarList) -> Vec<Entry> {
        list.entries.borrow().clone()
    }

    #[test]
    fn sync_splices_only_the_changed_middle() {
        let list = SidebarList::default();
        let [a, b, c, d] = [(); 4].map(|_| SessionId::new());
        let first = vec![Entry::LocalHeader, Entry::Local(a), Entry::Local(b), Entry::Local(c), Entry::LocalMore];
        list.sync(first.clone(), |_| None);
        assert_eq!(list.state.item_count(), 5);
        let second = vec![Entry::LocalHeader, Entry::Local(a), Entry::Local(d), Entry::Local(c), Entry::Local(b), Entry::LocalMore];
        list.sync(second.clone(), |_| None);
        assert_eq!(list.state.item_count(), 6);
        assert_eq!(ids(&list), second);
        assert_eq!(list.index(&Entry::Local(b)), Some(4));
        list.sync(vec![Entry::LocalHeader, Entry::LocalEmpty], |_| None);
        assert_eq!(list.state.item_count(), 2);
        assert!(list.visible_sessions().is_empty());
    }
}
