use super::*;
use crate::{
    root::peer::session_dialog::Editor,
    sidebar::{RowFocus, list::Entry},
};
use gpui::{FocusHandle, KeyDownEvent};
use shellfox_service::activity::AgentState;

pub(crate) struct PeerSectionLayout {
    generation: u64,
    ids: Vec<m::Id>,
    has_more: bool,
    restore_more_focus: bool,
    pub heading: FocusHandle,
    pub more: FocusHandle,
    row_focus: HashMap<m::Id, RowFocus>,
}
impl PeerSectionLayout {
    fn new(cx: &mut gpui::App) -> Self {
        Self {
            generation: 0,
            ids: Vec::new(),
            has_more: false,
            restore_more_focus: false,
            heading: cx.focus_handle().tab_stop(true),
            more: cx.focus_handle().tab_stop(true),
            row_focus: HashMap::new(),
        }
    }
    fn focus(&mut self, id: m::Id, cx: &mut gpui::App) -> RowFocus {
        self.row_focus
            .entry(id)
            .or_insert_with(|| RowFocus::new(cx))
            .clone()
    }
}

impl Root {
    /// Records the shown rows of one peer section and returns its heading focus.
    pub(super) fn record_peer_layout(
        &self,
        cycle: &source::PeerCycle,
        ids: Vec<m::Id>,
        has_more: bool,
        window: &Window,
        cx: &mut gpui::App,
    ) -> FocusHandle {
        let mut layouts = self.peer_ui.sidebar_layout.borrow_mut();
        let layout = layouts
            .entry(cycle.peer.clone())
            .or_insert_with(|| PeerSectionLayout::new(cx));
        layout.generation = cycle.generation;
        layout.has_more = has_more;
        let visible: HashSet<_> = ids.iter().copied().collect();
        layout.row_focus.retain(|id, _| visible.contains(id));
        for id in &ids {
            layout.focus(*id, cx);
        }
        layout.ids = ids;
        if std::mem::take(&mut layout.restore_more_focus)
            && !has_more
            && let Some(last) = layout.ids.last().copied()
        {
            // "Show more" disappeared under focus; move focus to the last row.
            let focus = layout.focus(last, cx).copy;
            let list = self.sidebar_ui.list.clone();
            let entry = Entry::Peer(cycle.peer.clone(), last);
            window.on_next_frame(move |window, _| {
                focus.focus(window);
                list.reveal(&entry);
            });
        }
        layout.heading.clone()
    }

    pub(crate) fn peer_row_focus(&self, peer: &AuthenticatedPeerId, id: m::Id) -> Option<FocusHandle> {
        let layouts = self.peer_ui.sidebar_layout.borrow();
        Some(layouts.get(peer)?.row_focus.get(&id)?.row.clone())
    }

    pub(super) fn render_peer_row(
        &self,
        cycle: &source::PeerCycle,
        session: &m::Session,
        dot: AgentState,
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(focus) = self
            .peer_ui
            .sidebar_layout
            .borrow_mut()
            .get_mut(&cycle.peer)
            .map(|layout| layout.focus(session.id, cx))
        else {
            return div().into_any_element();
        };
        let selected = self.peers.selected_peer().is_some_and(|(peer, state)| {
            peer == &cycle.peer && state.selected_session == Some(session.id)
        });
        self.peer_sidebar_row(cycle, session, dot, selected, focus, palette, window, cx)
    }

    pub(super) fn render_peer_more(
        &self,
        cycle: &source::PeerCycle,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(more) = self
            .peer_ui
            .sidebar_layout
            .borrow()
            .get(&cycle.peer)
            .map(|layout| layout.more.clone())
        else {
            return div().into_any_element();
        };
        let target = cycle.clone();
        div()
            .id(gpui::SharedString::from(format!(
                "peer-more-{}",
                cycle.peer.as_str()
            )))
            .track_focus(&more)
            .mx(px(12.))
            .my(px(4.))
            .p(px(4.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(palette.muted)
            .cursor_pointer()
            .hover(|style| style.text_color(palette.accent))
            .child(div().relative().top(px(1.)).child("Show more"))
            .on_click(cx.listener(move |root, _, window, cx| {
                root.show_more_peer_sessions(&target, window, cx)
            }))
            .into_any_element()
    }

    fn show_more_peer_sessions(
        &mut self,
        cycle: &source::PeerCycle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.peers.current(cycle) {
            return;
        }
        let pages = self.peer_ui.pages.entry(cycle.peer.clone()).or_insert(1);
        *pages = pages.saturating_add(1);
        if let Some(layout) = self
            .peer_ui
            .sidebar_layout
            .borrow_mut()
            .get_mut(&cycle.peer)
        {
            layout.restore_more_focus = layout.more.is_focused(window);
        }
        cx.notify();
    }

    pub(super) fn peer_row_navigation(
        &mut self,
        cycle: &source::PeerCycle,
        id: m::Id,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.peers.current(cycle) {
            return false;
        }
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        if mods.alt || mods.control || mods.platform {
            return false;
        }
        let editing = matches!(&self.dialog, Some(Dialog::PeerSession(dialog)) if dialog.cycle == *cycle && dialog.id == id && matches!(&dialog.editor, Editor::Rename(field) if field.read(cx).edit_focus().is_focused(window)));
        let mut layouts = self.peer_ui.sidebar_layout.borrow_mut();
        let Some(layout) = layouts.get_mut(&cycle.peer) else {
            return false;
        };
        if layout.generation != cycle.generation {
            return false;
        }
        let Some(index) = layout.ids.iter().position(|current| *current == id) else {
            return false;
        };
        let RowFocus { copy, settle, .. } = layout.focus(id, cx);
        let on_settle = settle.is_focused(window);
        let enabled = !self.peer_ui.pending.contains(&cycle.peer);
        if key == "tab" && on_settle && mods.shift {
            if let Some(Dialog::PeerSession(dialog)) = &self.dialog
                && dialog.cycle == *cycle
                && dialog.id == id
                && let Editor::Rename(field) = &dialog.editor
            {
                field.read(cx).focus(window);
            } else {
                copy.focus(window);
            }
            return true;
        }
        if key == "tab" && !on_settle && !mods.shift && enabled {
            settle.focus(window);
            return true;
        }
        let step = match key {
            "tab" if mods.shift => index.checked_sub(1),
            "tab" => (index + 1 < layout.ids.len()).then_some(index + 1),
            "home" if !editing => Some(0),
            "end" if !editing => layout.ids.len().checked_sub(1),
            "up" if !editing => index.checked_sub(1),
            "down" if !editing => (index + 1 < layout.ids.len()).then_some(index + 1),
            _ => return false,
        };
        if let Some(next) = step {
            let target = layout.ids[next];
            let RowFocus { copy, settle, .. } = layout.focus(target, cx);
            if key == "tab" && mods.shift && enabled {
                settle.focus(window);
            } else if let Some(Dialog::PeerSession(dialog)) = &self.dialog
                && dialog.cycle == *cycle
                && dialog.id == target
                && let Editor::Rename(field) = &dialog.editor
            {
                field.read(cx).focus(window);
            } else {
                copy.focus(window);
            }
            self.sidebar_ui.list.reveal(&Entry::Peer(cycle.peer.clone(), target));
            return true;
        }
        if key == "tab" && mods.shift {
            layout.heading.focus(window);
            return true;
        }
        if key == "tab" && layout.has_more {
            layout.more.focus(window);
            self.sidebar_ui.list.reveal(&Entry::PeerMore(cycle.peer.clone()));
            return true;
        }
        if key == "tab" {
            window.focus_next();
            return true;
        }
        false
    }

    pub(super) fn reveal_peer_session(&self, cycle: &source::PeerCycle, id: m::Id) {
        if !self.sidebar {
            return;
        }
        if let Some(layout) = self.peer_ui.sidebar_layout.borrow().get(&cycle.peer)
            && layout.generation == cycle.generation
            && layout.ids.contains(&id)
        {
            self.sidebar_ui.list.reveal(&Entry::Peer(cycle.peer.clone(), id));
        }
    }
}
