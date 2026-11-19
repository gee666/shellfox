use super::*;
use crate::{
    root::peer::session_dialog::Editor,
    sidebar::windowed::{ROW_HEIGHT, mounted_rows_at, reveal_at},
};
use gpui::{Div, FocusHandle, KeyDownEvent, Stateful};
use shellfox_service::activity::AgentState;

#[derive(Clone)]
pub(crate) struct RowFocus(pub FocusHandle, pub FocusHandle);

pub(crate) struct PeerSectionLayout {
    generation: u64,
    start: f32,
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
            start: 0.,
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
            .or_insert_with(|| {
                RowFocus(
                    cx.focus_handle().tab_stop(true),
                    cx.focus_handle().tab_stop(true),
                )
            })
            .clone()
    }
}

impl Root {
    pub(super) fn record_peer_layout(
        &self,
        cycle: &source::PeerCycle,
        start: f32,
        ids: Vec<m::Id>,
        has_more: bool,
        cx: &mut gpui::App,
    ) -> FocusHandle {
        let mut layouts = self.peer_ui.sidebar_layout.borrow_mut();
        let layout = layouts
            .entry(cycle.peer.clone())
            .or_insert_with(|| PeerSectionLayout::new(cx));
        layout.generation = cycle.generation;
        layout.start = start;
        layout.has_more = has_more;
        let visible: HashSet<_> = ids.iter().copied().collect();
        layout.row_focus.retain(|id, _| visible.contains(id));
        layout.ids = ids;
        layout.heading.clone()
    }

    pub(super) fn render_peer_page(
        &self,
        mut list: Stateful<Div>,
        cycle: &source::PeerCycle,
        rows: &[(&m::Session, AgentState)],
        palette: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut layouts = self.peer_ui.sidebar_layout.borrow_mut();
        let Some(layout) = layouts.get_mut(&cycle.peer) else {
            return list;
        };
        let scroll = &self.sidebar_ui.scroll;
        if std::mem::take(&mut layout.restore_more_focus)
            && !layout.has_more
            && let Some(last) = layout.ids.last().copied()
        {
            let focus = layout.focus(last, cx).0;
            let start = layout.start;
            let index = layout.ids.len() - 1;
            let scroll = scroll.clone();
            window.on_next_frame(move |window, _| {
                focus.focus(window);
                reveal_at(&scroll, start, index);
            });
        }
        let retained = rows.iter().enumerate().filter_map(|(index, (session, _))| {
            let id = session.id;
            let focused = layout.row_focus.get(&id).is_some_and(|RowFocus(copy, settle)| copy.is_focused(window) || settle.is_focused(window));
            let editing = matches!(&self.dialog, Some(Dialog::PeerSession(dialog)) if dialog.cycle == *cycle && dialog.id == id);
            let menu = self.session_menu.as_ref().is_some_and(|menu| menu.is_peer_row(cycle, id));
            (focused || editing || menu).then_some(index)
        });
        let measured_height = f32::from(scroll.bounds().size.height);
        let height = if measured_height > 0. {
            measured_height
        } else {
            f32::from(window.viewport_size().height) - 32.
        };
        let indices = mounted_rows_at(
            rows.len(),
            layout.start,
            -f32::from(scroll.offset().y),
            height,
            retained,
        );
        let mut cursor = 0;
        for index in indices {
            if index > cursor {
                list = list.child(div().h(px(ROW_HEIGHT * (index - cursor) as f32)));
            }
            let (session, dot) = rows[index];
            let selected = self.peers.selected_peer().is_some_and(|(peer, state)| {
                peer == &cycle.peer && state.selected_session == Some(session.id)
            });
            list = list.child(self.peer_sidebar_row(
                cycle,
                session,
                dot,
                selected,
                layout.focus(session.id, cx),
                palette,
                window,
                cx,
            ));
            cursor = index + 1;
        }
        if cursor < rows.len() {
            list = list.child(div().h(px(ROW_HEIGHT * (rows.len() - cursor) as f32)));
        }
        if layout.has_more {
            let target = cycle.clone();
            list = list.child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "peer-more-{}",
                        cycle.peer.as_str()
                    )))
                    .track_focus(&layout.more)
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
                    })),
            );
        }
        list
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
        let RowFocus(copy, settle) = layout.focus(id, cx);
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
            let RowFocus(copy, settle) = layout.focus(target, cx);
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
            reveal_at(&self.sidebar_ui.scroll, layout.start, next);
            return true;
        }
        if key == "tab" && mods.shift {
            layout.heading.focus(window);
            return true;
        }
        if key == "tab" && layout.has_more {
            layout.more.focus(window);
            crate::sidebar::windowed::reveal_region(&self.sidebar_ui.scroll, layout.start + ROW_HEIGHT * layout.ids.len() as f32 + 4., 24.);
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
            && let Some(index) = layout.ids.iter().position(|current| *current == id)
        {
            reveal_at(&self.sidebar_ui.scroll, layout.start, index);
        }
    }
}
