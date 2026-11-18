use gpui::{Div, ScrollHandle, Stateful, div, point, prelude::*, px};
use shellfox_host::{SessionId, TabId};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[path = "tab_scroll_math.rs"]
mod math;
use math::Viewport;

type LocalViewport = Viewport<SessionId, TabId>;

#[derive(Default, Clone)]
pub(crate) struct TabScroll {
    handle: ScrollHandle,
    last_viewport: Rc<Cell<Option<LocalViewport>>>,
    focus_handles: Rc<RefCell<HashMap<TabId, gpui::FocusHandle>>>,
}

impl TabScroll {
    pub(super) fn focus_handle(
        &self,
        tab: TabId,
        cx: &mut gpui::Context<crate::root::Root>,
    ) -> gpui::FocusHandle {
        self.focus_handles
            .borrow_mut()
            .entry(tab)
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }

    pub(super) fn retain_focus_handles(&self, tabs: &HashSet<TabId>) {
        self.focus_handles
            .borrow_mut()
            .retain(|tab, _| tabs.contains(tab));
    }

    pub(super) fn viewport(
        &self,
        session: SessionId,
        selected: Option<(TabId, usize)>,
    ) -> Stateful<Div> {
        let scroll = self.clone();
        div()
            .on_children_prepainted(move |_, window, _| {
                let bounds = scroll.handle.bounds();
                let viewport = Viewport {
                    session,
                    selected,
                    width: f32::from(bounds.size.width),
                };
                let offset = scroll.handle.offset();
                let previous = scroll.last_viewport.replace(Some(viewport));
                let tab_edges = selected
                    .and_then(|(_, index)| scroll.handle.bounds_for_item(index))
                    .map(|tab| {
                        (
                            f32::from(tab.left() - bounds.left()),
                            f32::from(tab.right() - bounds.left()),
                        )
                    });
                let x = viewport
                    .reveal_left(
                        previous.as_ref(),
                        -f32::from(offset.x),
                        tab_edges,
                        f32::from(scroll.handle.max_offset().width),
                    )
                    .map(|left| px(-left));
                if let Some(x) = x.filter(|x| *x != offset.x) {
                    // GPUI has already positioned the children for this frame.
                    scroll.handle.set_offset(point(x, offset.y));
                    window.request_animation_frame();
                }
            })
            .id("local-terminal-tabs")
            .h_full()
            .flex()
            .flex_shrink()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.handle)
    }
}
