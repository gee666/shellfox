use super::tab_reorder::{TabDrag, TabStrip};
use gpui::{
    Bounds, Div, Pixels, Point, ScrollHandle, Stateful, Window, div, point, prelude::*, px, size,
};
use shellfox_host::{SessionId, TabId};
use shellfox_core::TerminalIdentity;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[path = "tab_scroll_math.rs"]
mod math;
use math::Viewport;

type LocalViewport = Viewport<SessionId, TabId>;

#[derive(Clone, Copy)]
struct DragPosition {
    tab: TabId,
    point: Point<Pixels>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TabFocus {
    Label(TabId),
    Close(TerminalIdentity),
    NewTerminal,
    Files,
    Folder,
}

impl TabFocus {
    fn tab(self) -> Option<TabId> {
        match self {
            Self::Label(tab) | Self::Close(TerminalIdentity { tab_id: tab, .. }) => Some(tab),
            _ => None,
        }
    }
}

#[derive(Default, Clone)]
pub(crate) struct TabScroll {
    handle: ScrollHandle,
    last_viewport: Rc<Cell<Option<LocalViewport>>>,
    drag: Rc<Cell<Option<DragPosition>>>,
    focus_handles: Rc<RefCell<HashMap<TabFocus, gpui::FocusHandle>>>,
    pending_reveal: Rc<Cell<Option<TabId>>>,
    pub(super) files_pointer_focus: Rc<Cell<bool>>,
}

impl TabScroll {
    pub(super) fn focus_handle(
        &self,
        tab: TabId,
        cx: &mut gpui::Context<crate::root::Root>,
    ) -> gpui::FocusHandle {
        self.control_focus(TabFocus::Label(tab), cx)
    }

    pub(super) fn control_focus(&self, key: TabFocus, cx: &gpui::App) -> gpui::FocusHandle {
        self.focus_handles.borrow_mut().entry(key)
            .or_insert_with(|| cx.focus_handle().tab_index(0)).clone()
    }

    pub(super) fn controls(&self) -> Vec<TabFocus> {
        self.focus_handles.borrow().keys().copied().filter(|key| !matches!(key, TabFocus::Label(_))).collect()
    }

    pub(super) fn retain_focus_handles(&self, tabs: &HashSet<TabId>, generations: &HashSet<TerminalIdentity>) {
        self.focus_handles.borrow_mut().retain(|key, _| match key {
            TabFocus::Close(identity) => generations.contains(identity),
            TabFocus::Label(tab) => tabs.contains(tab),
            _ => true,
        });
    }

    pub(super) fn reveal_on_paint(&self, tab: TabId) {
        self.pending_reveal.set(Some(tab));
    }

    pub(super) fn focused_tab(&self, window: &Window) -> Option<TabId> {
        self.focus_handles.borrow().iter().find_map(|(key, handle)| {
            handle.is_focused(window).then(|| key.tab()).flatten()
        })
    }

    pub(super) fn windowed(count: usize) -> bool {
        count > math::WINDOW_THRESHOLD
    }

    pub(super) fn slot_width() -> Pixels {
        px(math::WINDOWED_WIDTH)
    }

    pub(super) fn visible_indices(
        &self,
        count: usize,
        retained: impl IntoIterator<Item = usize>,
    ) -> Vec<usize> {
        if !Self::windowed(count) {
            return (0..count).collect();
        }
        let bounds = self.handle.bounds();
        let width = f32::from(bounds.size.width);
        math::window_indices(
            count,
            -f32::from(self.handle.offset().x),
            if width > 0. { width } else { 800. },
            retained,
        )
    }

    pub(super) fn item_bounds(&self, index: usize, windowed: bool) -> Option<Bounds<Pixels>> {
        let mut bounds = if windowed {
            let viewport = self.handle.bounds();
            Bounds::new(
                viewport.origin + point(px(index as f32 * math::WINDOWED_WIDTH), px(0.)),
                size(Self::slot_width(), px(30.)),
            )
        } else {
            self.handle.bounds_for_item(index)?
        };
        bounds.origin += self.handle.offset();
        Some(bounds)
    }

    pub(super) fn focused_or_dragged(&self, window: &Window) -> [Option<TabId>; 2] {
        [self.focused_tab(window), self.drag.get().map(|drag| drag.tab)]
    }

    pub(super) fn viewport(
        &self,
        session: SessionId,
        selected: Option<(TabId, usize)>,
        tabs: Rc<[TabId]>,
        windowed: bool,
        strip: Option<TabStrip>,
    ) -> Stateful<Div> {
        let scroll = self.clone();
        let drag = self.drag.clone();
        let previous_display = (self.handle.offset().x, self.handle.bounds().size.width);
        div()
            .on_children_prepainted(move |_, window, cx| {
                let bounds = scroll.handle.bounds();
                let viewport = Viewport {
                    session,
                    selected,
                    width: f32::from(bounds.size.width),
                };
                let offset = scroll.handle.offset();
                let previous = scroll.last_viewport.replace(Some(viewport));
                let tab_edges = |index| scroll.item_bounds(index, windowed).map(|tab| {
                        (
                            f32::from(tab.left() - bounds.left() - offset.x),
                            f32::from(tab.right() - bounds.left() - offset.x),
                        )
                    });
                let max_left = f32::from(scroll.handle.max_offset().width);
                let left = -f32::from(offset.x);
                let focus_index = scroll.pending_reveal.take()
                    .and_then(|id| tabs.iter().position(|&tab| tab == id));
                let next_left = if let Some(index) = focus_index {
                    math::reveal_tab(left, tab_edges(index), viewport.width, max_left)
                } else {
                    viewport.reveal_left(previous.as_ref(), left, selected.and_then(|(_, index)| tab_edges(index)), max_left).unwrap_or(left)
                };
                let mut x = px(-next_left);
                if !cx.has_active_drag() {
                    scroll.drag.set(None);
                } else if let Some(position) = scroll
                    .drag
                    .get()
                    .map(|drag| drag.point)
                    .filter(|position| bounds.contains(position))
                {
                    let edge = if position.x < bounds.left() + px(32.) {
                        18.
                    } else if position.x > bounds.right() - px(32.) {
                        -18.
                    } else {
                        0.
                    };
                    x = (x + px(edge)).clamp(-scroll.handle.max_offset().width, px(0.));
                }
                if x != offset.x {
                    scroll.handle.set_offset(point(x, offset.y));
                }
                if x != offset.x || windowed && previous_display != (x, bounds.size.width) {
                    // Rebuild the mounted range after layout computes the actual viewport.
                    window.request_animation_frame();
                }
            })
            .id("local-terminal-tabs")
            .on_drag_move(move |event: &gpui::DragMoveEvent<TabDrag>, window, cx| {
                let current = event.drag(cx);
                if Some(current.strip) == strip {
                    drag.set(Some(DragPosition {
                        tab: current.tab,
                        point: event.event.position,
                    }));
                    window.refresh();
                }
            })
            // Tabs cover the strip's bottom border, as in terminal.css.
            .h(px(30.))
            .mt(px(1.))
            .flex()
            .flex_shrink()
            .min_w_0()
            .when(windowed, |strip| strip.flex_1())
            .overflow_x_scroll()
            .track_scroll(&self.handle)
    }
}
