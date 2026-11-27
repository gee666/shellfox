use crate::{
    assets::{icon_button, icon_sized, toolbar_tip, tooltip},
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, FocusHandle, Window, div, prelude::*, px};
use shellfox_core::{Session, SessionId, SessionOrder, Snapshot, compare_session_order};
use shellfox_service::activity::{AgentState, agent_state_rank};
use std::collections::HashMap;

mod width;
pub(crate) use width::{SidebarDrag, clamp_width};
mod actions;
pub(crate) mod list;
mod new_session_menu;
pub(crate) mod row;
mod toolbar;

/// Focus for one sidebar row: the title button, the settle button, and the
/// whole row, which keeps the row rendered while either one has focus.
#[derive(Clone)]
pub(crate) struct RowFocus {
    pub(crate) copy: FocusHandle,
    pub(crate) settle: FocusHandle,
    pub(crate) row: FocusHandle,
}

impl RowFocus {
    pub(crate) fn new(cx: &mut gpui::App) -> Self {
        Self {
            copy: cx.focus_handle().tab_stop(true),
            settle: cx.focus_handle().tab_stop(true),
            row: cx.focus_handle(),
        }
    }
}

pub(crate) struct SidebarUi {
    pub(crate) visible: bool,
    pub(crate) width: f32,
    pub(crate) resizing: bool,
    pub(crate) local_expanded: bool,
    pub(crate) history_page_size: usize,
    /// Pages of local session history shown.
    pub(crate) session_pages: usize,
    pub(crate) session_menu: Option<session_menu::SessionMenu>,
    pub(crate) visibility: crate::workspace::visibility::SessionVisibility,
    row_focus: HashMap<SessionId, RowFocus>,
    pub(crate) list: list::SidebarList,
    scrollbar: crate::files::scrollbar::Scrollbar,
    group: FocusHandle,
    more: FocusHandle,
    new_session: FocusHandle,
    target_menu: Option<new_session_menu::NewSessionMenu>,
    new_session_anchor: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub(crate) settings: FocusHandle,
    pub(crate) region: FocusHandle,
    pub(crate) toggle: FocusHandle,
    pub(crate) resizer: FocusHandle,
    pub(crate) delete_cancel: FocusHandle,
    pub(crate) environment_resize: crate::dialogs::environment::resize::EnvironmentResize,
    pub(crate) resize_origin: Option<SidebarDrag>,
    pub(crate) keyboard_focus: bool,
}

impl SidebarUi {
    pub(crate) fn new_session_menu_open(&self) -> bool {
        self.target_menu.is_some()
    }

    pub(crate) fn new(cx: &mut gpui::App) -> Self {
        Self {
            visible: true,
            width: 220.,
            resizing: false,
            local_expanded: true,
            history_page_size: 10,
            session_pages: 1,
            session_menu: None,
            visibility: Default::default(),
            row_focus: HashMap::new(),
            list: Default::default(),
            scrollbar: Default::default(),
            group: cx.focus_handle().tab_stop(true),
            more: cx.focus_handle().tab_stop(true),
            new_session: cx.focus_handle().tab_stop(true),
            target_menu: None,
            new_session_anchor: Default::default(),
            settings: cx.focus_handle().tab_stop(true),
            region: cx.focus_handle(),
            toggle: cx.focus_handle().tab_stop(true),
            resizer: cx.focus_handle().tab_stop(true),
            delete_cancel: cx.focus_handle(),
            environment_resize: Default::default(),
            resize_origin: None,
            keyboard_focus: false,
        }
    }

    pub(crate) fn menu_open(&self) -> bool {
        self.target_menu.is_some()
    }

    pub(crate) fn snapshot(&mut self, snapshot: &Snapshot, cx: &mut gpui::App) {
        let old = std::mem::take(&mut self.row_focus);
        let mut old = old;
        self.row_focus = snapshot
            .sessions
            .iter()
            .map(|session| {
                let id = session.id;
                (id, old.remove(&id).unwrap_or_else(|| RowFocus::new(cx)))
            })
            .collect();
    }

    pub(crate) fn focus_row(&self, id: SessionId, window: &mut gpui::Window) {
        if let Some(focus) = self.row_focus.get(&id) {
            focus.copy.focus(window);
        }
    }
}

/// Local and peer rows share one order, ranked by the dot each row shows.
pub(crate) fn session_order<'a>(
    pinned: bool,
    state: AgentState,
    created_at: &'a str,
    title: &'a str,
    id: SessionId,
) -> SessionOrder<'a> {
    SessionOrder {
        pinned,
        rank: agent_state_rank(state),
        created_at,
        title,
        id,
    }
}

fn sort_sessions(
    sessions: &[Session],
    mut state: impl FnMut(&Session) -> AgentState,
) -> Vec<(&Session, AgentState)> {
    let mut rows: Vec<_> = sessions
        .iter()
        .map(|session| (session, state(session)))
        .collect();
    rows.sort_by(|(a, a_state), (b, b_state)| {
        compare_session_order(&local_order(a, *a_state), &local_order(b, *b_state))
    });
    rows
}

fn local_order(s: &Session, state: AgentState) -> SessionOrder<'_> {
    session_order(s.pinned_at.is_some(), state, &s.created_at, &s.title, s.id)
}

impl Root {
    fn sidebar_rows(&self, cx: &gpui::App) -> Vec<(&Session, AgentState)> {
        self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            sort_sessions(&snapshot.sessions, |session| {
                self.session_agent_state(session, cx)
            })
        })
    }

    pub(crate) fn session_sidebar(
        &self,
        palette: Palette,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sidebar.list.begin_frame();
        let list = self
            .sidebar_list(palette, window, cx)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .py(px(4.));
        let state = self.sidebar.list.state().clone();
        let scrolling = state.max_offset_for_scrollbar().height > px(0.);
        let weak = cx.weak_entity();
        let list = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .child(list.when(scrolling, |list| list.mr(px(10.))))
            .when(scrolling, |frame| {
                frame.child(self.sidebar.scrollbar.thin(&state, palette))
            })
            .child(self.sidebar.list.painted_marker())
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |_, (), window, cx| {
                        if scrolling != (state.max_offset_for_scrollbar().height > px(0.)) {
                            window.defer(cx, move |_, cx| {
                                let _ = weak.update(cx, |_, cx| cx.notify());
                            });
                        }
                    },
                )
                .absolute()
                .inset_0(),
            );
        let width = clamp_width(self.sidebar.width, f32::from(window.viewport_size().width));
        let mut sidebar = div()
            .relative()
            .track_focus(&self.sidebar.region)
            .capture_key_down(cx.listener(|root, _: &gpui::KeyDownEvent, _, cx| {
                if !root.sidebar.keyboard_focus {
                    root.sidebar.keyboard_focus = true;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|root, _, _, cx| {
                    if root.sidebar.keyboard_focus {
                        root.sidebar.keyboard_focus = false;
                        cx.notify();
                    }
                }),
            )
            .w(px(width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.sidebar)
            .child(self.sidebar_toolbar(palette, cx))
            .child(list)
            .children(self.crash_notice(palette, cx))
            .children(self.update_notice(palette, cx));
        if let Some(crate::dialogs::Dialog::DeleteSession(id)) = self.dialogs.current
            && let Some(session) = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
        {
            sidebar = sidebar.child(self.session_delete_confirmation(session, palette, cx));
        }
        if let Some(confirmation) = self.render_peer_session_delete(palette, cx) {
            sidebar = sidebar.child(confirmation);
        }
        sidebar.into_any_element()
    }

    pub(crate) fn apply_history_page_size(&mut self, size: usize) -> bool {
        if self.sidebar.history_page_size == size {
            return false;
        }
        self.sidebar.history_page_size = size;
        self.sidebar.session_pages = 1;
        self.peer_ui.pages.clear();
        true
    }

    pub(crate) fn toggle_local_group(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_local_source() {
            self.select_local_source(_window, cx);
        }
        self.sidebar.local_expanded = !self.sidebar.local_expanded;
        self.hide_settings(cx);
        cx.notify();
    }

    fn show_more_sessions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let old_end = self
            .sidebar
            .session_pages
            .saturating_mul(self.sidebar.history_page_size);
        self.sidebar.session_pages = self.sidebar.session_pages.saturating_add(1);
        let rows = self.sidebar_rows(cx);
        let end = self
            .sidebar
            .session_pages
            .saturating_mul(self.sidebar.history_page_size);
        if end >= rows.len()
            && old_end < rows.len()
            && self.sidebar.more.is_focused(window)
            && let Some((last, _)) = rows.last()
        {
            self.sidebar.focus_row(last.id, window);
            self.sidebar.list.reveal(&list::Entry::Local(last.id));
        }
        cx.notify();
    }
}

pub(crate) mod session_menu;
#[cfg(test)]
mod tests;
