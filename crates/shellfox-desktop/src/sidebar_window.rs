use super::*;
use gpui::{Bounds, Div, Pixels, Stateful, point, size};
use std::collections::BTreeSet;

pub(crate) const ROW_HEIGHT: f32 = 36.;
const FIRST_ROW: f32 = 32.; // 4px list padding and 28px Local header.
const OVERSCAN: usize = 5;
const WINDOW_THRESHOLD: usize = 100;

pub(super) fn mounted_rows(
    count: usize,
    scroll_y: f32,
    height: f32,
    retained: impl IntoIterator<Item = usize>,
) -> Vec<usize> {
    mounted_rows_at(count, FIRST_ROW, scroll_y, height, retained)
}

pub(crate) fn mounted_rows_at(
    count: usize,
    row_start: f32,
    scroll_y: f32,
    height: f32,
    retained: impl IntoIterator<Item = usize>,
) -> Vec<usize> {
    if count <= WINDOW_THRESHOLD || height <= 0. {
        return (0..count).collect();
    }
    let top = scroll_y - row_start;
    let first = ((top / ROW_HEIGHT).floor() - OVERSCAN as f32).clamp(0., count as f32) as usize;
    let last = (((top + height) / ROW_HEIGHT).ceil() + OVERSCAN as f32)
        .clamp(first as f32, count as f32) as usize;
    let mut indices: BTreeSet<_> = (first..last).collect();
    indices.extend(retained.into_iter().filter(|index| *index < count));
    indices.extend([0, count - 1]);
    indices.into_iter().collect()
}

pub(super) fn reveal(scroll: &ScrollHandle, index: usize) {
    reveal_at(scroll, FIRST_ROW, index);
}

pub(crate) fn reveal_at(scroll: &ScrollHandle, row_start: f32, index: usize) {
    reveal_region(scroll, row_start + ROW_HEIGHT * index as f32, ROW_HEIGHT);
}

pub(crate) fn reveal_region(scroll: &ScrollHandle, top: f32, extent: f32) {
    let mut offset = scroll.offset();
    let top = px(top);
    let height = scroll.bounds().size.height;
    if height <= px(0.) {
        return;
    }
    if top + offset.y < px(0.) {
        offset.y = -top;
    } else if top + offset.y + px(extent) > height {
        offset.y = height - top - px(extent);
    }
    scroll.set_offset(offset);
}

pub(super) fn bounds(scroll: &ScrollHandle, index: usize) -> Bounds<Pixels> {
    let bounds = scroll.bounds();
    Bounds::new(
        point(
            bounds.origin.x,
            bounds.origin.y + scroll.offset().y + px(FIRST_ROW + ROW_HEIGHT * index as f32),
        ),
        size(bounds.size.width, px(ROW_HEIGHT)),
    )
}

impl Root {
    pub(super) fn render_sidebar_page(
        &self,
        mut list: Stateful<Div>,
        shown: usize,
        palette: Palette,
        window: &gpui::Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        use crate::root::Dialog;
        let rows = self.sidebar_rows(cx);
        let count = rows.len().min(shown);
        let scroll = &self.sidebar_ui.scroll;
        let retained = rows[..count].iter().enumerate().filter_map(|(index, (session, _))| {
            let id = session.id;
            let focused = self.sidebar_ui.row_focus.get(&id).is_some_and(|(row, settle)| row.is_focused(window) || settle.is_focused(window));
            let dialog = matches!(&self.dialog, Some(Dialog::RenameSession { id: target, .. } | Dialog::DeleteSession(target) | Dialog::SessionEnvironment { id: target, .. }) if *target == id);
            let menu = self.session_menu.as_ref().and_then(|menu| menu.local_id()) == Some(id);
            (focused || dialog || menu).then_some(index)
        });
        let indices = mounted_rows(
            count,
            -f32::from(scroll.offset().y),
            f32::from(scroll.bounds().size.height),
            retained,
        );
        let mut cursor = 0;
        for i in indices {
            if i > cursor {
                list = list.child(div().h(px(ROW_HEIGHT * (i - cursor) as f32)));
            }
            let (session, dot) = rows[i];
            list = list.child(self.session_row(session, dot, palette, window, cx));
            cursor = i + 1;
        }
        if cursor < count {
            list = list.child(div().h(px(ROW_HEIGHT * (count - cursor) as f32)));
        }
        list
    }

    pub(super) fn sidebar_focus_tab(
        &self,
        id: SessionId,
        shift: bool,
        window: &mut gpui::Window,
        cx: &gpui::App,
    ) {
        use crate::root::Dialog;
        let Some((_, settle)) = self.sidebar_ui.row_focus.get(&id) else {
            return;
        };
        let focus_copy = |target: SessionId, window: &mut gpui::Window| {
            if let Some(Dialog::RenameSession { id, title }) = &self.dialog
                && *id == target
            {
                title.read(cx).focus(window);
            } else {
                self.sidebar_ui.focus_row(target, window);
            }
        };
        let on_settle = settle.is_focused(window);
        if on_settle && shift {
            focus_copy(id, window);
            return;
        }
        if !on_settle && !shift && !self.user_action_pending {
            settle.focus(window);
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
                && let Some((_, settle)) = self.sidebar_ui.row_focus.get(&target)
            {
                settle.focus(window);
            } else {
                focus_copy(target, window);
            }
            reveal(&self.sidebar_ui.scroll, next);
        } else if shift {
            self.sidebar_ui.group.focus(window);
        } else if shown < rows.len() {
            self.sidebar_ui.more.focus(window);
            reveal_region(&self.sidebar_ui.scroll, FIRST_ROW + ROW_HEIGHT * shown as f32 + 4., 24.);
        } else {
            window.focus_next();
        }
    }

    pub(crate) fn reveal_sidebar_session(&self, id: SessionId, cx: &gpui::App) {
        if !self.sidebar || !self.local_expanded {
            return;
        }
        if let Some(index) = self
            .sidebar_rows(cx)
            .iter()
            .take(self.session_pages.saturating_mul(self.history_page_size))
            .position(|(session, _)| session.id == id)
        {
            reveal(&self.sidebar_ui.scroll, index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peer_group_below_the_viewport_only_keeps_boundaries_and_retained_rows() {
        assert_eq!(mounted_rows_at(250, 1_000., 0., 480., [42]), [0, 42, 249]);
        assert_eq!(
            mounted_rows_at(250, 1_000., 20_000., 480., [42]),
            [0, 42, 249]
        );
    }

    #[test]
    fn offscreen_rows_keep_only_boundaries_overscan_and_retained_targets() {
        assert_eq!(
            mounted_rows(100, 0., 360., []),
            (0..100).collect::<Vec<_>>()
        );
        let window = mounted_rows(
            400,
            FIRST_ROW + 100. * ROW_HEIGHT,
            ROW_HEIGHT * 10.,
            [3, 370],
        );
        assert_eq!(
            window,
            [vec![0, 3], (95..115).collect(), vec![370, 399]].concat()
        );
    }
}
