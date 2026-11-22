use crate::{
    assets::{icon_button, icon_sized, toolbar_tip, tooltip},
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, FocusHandle, ScrollHandle, div, prelude::*, px};
use shellfox_core::{Session, SessionId, Snapshot};
use shellfox_service::activity::AgentState;
use std::collections::HashMap;

#[path = "sidebar_width.rs"]
mod width;
pub(crate) use width::{SidebarDrag, clamp_width};
#[path = "sidebar_actions.rs"]
mod actions;
#[path = "new_session_menu.rs"]
mod new_session_menu;
#[path = "sidebar_row.rs"]
pub(crate) mod row;
#[path = "sidebar_toolbar.rs"]
mod toolbar;
#[path = "sidebar_window.rs"]
pub(crate) mod windowed;

pub(crate) struct SidebarUi {
    pub(crate) visibility: crate::session_visibility::SessionVisibility,
    row_focus: HashMap<SessionId, (FocusHandle, FocusHandle)>,
    pub(crate) scroll: ScrollHandle,
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
    pub(crate) environment_resize: crate::session_environment::resize::EnvironmentResize,
    pub(crate) resize_origin: Option<SidebarDrag>,
    pub(crate) keyboard_focus: bool,
}

impl SidebarUi {
    pub(crate) fn new(cx: &mut gpui::App) -> Self {
        Self {
            visibility: Default::default(),
            row_focus: HashMap::new(),
            scroll: ScrollHandle::new(),
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
                (
                    id,
                    old.remove(&id).unwrap_or_else(|| {
                        (
                            cx.focus_handle().tab_stop(true),
                            cx.focus_handle().tab_stop(true),
                        )
                    }),
                )
            })
            .collect();
    }

    pub(crate) fn focus_row(&self, id: SessionId, window: &mut gpui::Window) {
        if let Some((focus, _)) = self.row_focus.get(&id) {
            focus.focus(window);
        }
    }
}

pub(crate) fn rank(state: AgentState) -> u8 {
    match state {
        AgentState::Error => 0,
        AgentState::Running => 1,
        AgentState::Busy => 2,
        AgentState::Shell => 3,
        AgentState::Settled => 4,
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
        b.pinned_at
            .is_some()
            .cmp(&a.pinned_at.is_some())
            .then_with(|| a.pinned_at.cmp(&b.pinned_at))
            .then_with(|| rank(*a_state).cmp(&rank(*b_state)))
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    rows
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
        let mut list = div()
            .id("sessions")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_x_hidden()
            .overflow_y_scroll()
            .track_scroll(&self.sidebar_ui.scroll)
            .py(px(4.));
        if let Some(snapshot) = &self.snapshot {
            let count = snapshot.sessions.len();
            let shown = self.session_pages.saturating_mul(self.history_page_size);
            list = list.child(self.workspace_button("workspace/local/sessions", "Local sessions",
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
                        if self.local_expanded {
                            "down"
                        } else {
                            "chevron"
                        },
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
                    .on_click(
                        cx.listener(|root, _, window, cx| root.toggle_local_group(window, cx)),
                    ),
            ));
            if self.local_expanded {
                list = self.render_sidebar_page(list, shown, palette, window, cx);
                if shown < count {
                    list = list.child(
                        div().flex().mx(px(12.)).my(px(4.)).child(
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
                                .on_click(cx.listener(|root, _, window, cx| {
                                    root.show_more_sessions(window, cx)
                                })),
                        ),
                    );
                }
                if count == 0 {
                    list = list.child(
                        div()
                            .mx(px(8.))
                            .my(px(12.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(palette.muted)
                            .child("No sessions, press +"),
                    );
                }
            }
        }
        if self.snapshot.is_some() && self.peers.peers.values().any(|peer| peer.remembered) {
            list = list.child(div().h(px(4.)));
        }
        list = self.peer_sidebar(list, palette, window, cx);
        let scroll = self.sidebar_ui.scroll.clone();
        let scrolling = scroll.max_offset().height > px(0.);
        let weak = cx.weak_entity();
        let list = div().relative().flex_1().min_h_0().flex()
            .child(list.when(scrolling, |list| list.mr(px(10.))))
            .when(scrolling, |frame| frame.child(self.sidebar_ui.scrollbar.thin(&scroll, palette)))
            .child(gpui::canvas(|_, _, _| (), move |_, (), window, cx| {
                if scrolling != (scroll.max_offset().height > px(0.)) {
                    window.defer(cx, move |_, cx| { let _ = weak.update(cx, |_, cx| cx.notify()); });
                }
            }).absolute().inset_0());
        let width = clamp_width(self.sidebar_width, f32::from(window.viewport_size().width));
        let mut sidebar = div()
            .relative()
            .track_focus(&self.sidebar_ui.region)
            .capture_key_down(cx.listener(|root, _: &gpui::KeyDownEvent, _, cx| {
                if !root.sidebar_ui.keyboard_focus {
                    root.sidebar_ui.keyboard_focus = true;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|root, _, _, cx| {
                    if root.sidebar_ui.keyboard_focus {
                        root.sidebar_ui.keyboard_focus = false;
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
            .children(self.update_notice(palette, cx));
        if let Some(crate::root::Dialog::DeleteSession(id)) = self.dialog
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
        if self.history_page_size == size {
            return false;
        }
        self.history_page_size = size;
        self.session_pages = 1;
        self.peer_ui.pages.clear();
        true
    }

    pub(crate) fn first_peer_section_y(&self) -> f32 {
        let Some(snapshot) = &self.snapshot else {
            return 4.;
        };
        let count = snapshot.sessions.len();
        let shown = count.min(self.session_pages.saturating_mul(self.history_page_size));
        let mut height = 36.; // List padding, Local header, and trailing section padding.
        if self.local_expanded {
            height += shown as f32 * windowed::ROW_HEIGHT;
            if count == 0 {
                height += 40.;
            }
            if shown < count {
                height += 32.;
            }
        }
        height
    }

    pub(crate) fn toggle_local_group(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) {
        if !self.is_local_source() {
            self.select_local_source(_window, cx);
        }
        self.local_expanded = !self.local_expanded;
        self.hide_settings(cx);
        cx.notify();
    }

    fn show_more_sessions(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let old_end = self.session_pages.saturating_mul(self.history_page_size);
        self.session_pages = self.session_pages.saturating_add(1);
        let rows = self.sidebar_rows(cx);
        let end = self.session_pages.saturating_mul(self.history_page_size);
        if end >= rows.len()
            && old_end < rows.len()
            && self.sidebar_ui.more.is_focused(window)
            && let Some((last, _)) = rows.last()
        {
            self.sidebar_ui.focus_row(last.id, window);
            windowed::reveal_region(&self.sidebar_ui.scroll, 32. + windowed::ROW_HEIGHT * (rows.len() - 1) as f32, windowed::ROW_HEIGHT + 4.);
        }
        cx.notify();
    }
}

#[cfg(test)]
#[path = "sidebar_tests.rs"]
mod tests;
