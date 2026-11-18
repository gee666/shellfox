use crate::{
    accessibility::{Role, SemanticNode},
    root::{Dialog, Root},
    theme::Palette,
};
use gpui::{AnyElement, Context, Div, Render, Window, div, prelude::*, px};
use shellfox_core::TabState;
use std::collections::HashSet;

#[path = "tab_reorder.rs"]
mod tab_reorder;
#[path = "tab_scroll.rs"]
mod tab_scroll;
pub(crate) use tab_scroll::TabScroll;

pub(crate) fn button(
    id: impl Into<gpui::SharedString>,
    label: impl Into<gpui::SharedString>,
    palette: Palette,
) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .px_2()
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(12.))
        .rounded_sm()
        .cursor_pointer()
        .text_color(palette.muted)
        .hover(move |style| style.bg(palette.hover).text_color(palette.accent))
        .child(label.into())
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let resize_root = cx.entity();
        let motion = gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _cx| {
                let root = resize_root.clone();
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        root.update(cx, |root, cx| root.resize_layout(event, window, cx));
                    }
                });
            },
        )
        .absolute()
        .inset_0();
        let palette = self
            .snapshot
            .as_ref()
            .map(|s| Palette::new(&s.preferences.accent_color, &s.preferences.background_color))
            .unwrap_or_else(|| Palette::new("#ec4899", "#111016"));
        self.sync_field_palette(palette, cx);
        self.sync_dialog_edit_state(cx);
        self.sync_semantic_children(cx);
        if let Some(fixture) = &self.fixture {
            return self.finish_semantic_root(fixture.clone(), cx);
        }
        let mut content = div()
            .relative()
            .flex()
            .size_full()
            .overflow_hidden()
            .bg(palette.background)
            .text_color(palette.foreground)
            .font_family(if cfg!(windows) {
                "Segoe UI"
            } else {
                ".SystemUIFont"
            })
            .text_size(px(13.))
            .child(motion);
        content = content.child(
            if self
                .files
                .as_ref()
                .is_some_and(|files| files.visible && files.maximized)
            {
                self.files_panel(palette, window, cx)
            } else {
                self.layout_body(palette, window, cx)
            },
        );
        if self.closing_window {
            content = content.child(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .track_focus(&self.modal_focus)
                    .on_key_down(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    ),
            );
        }
        if let Some(message) = self.error.clone() {
            content = content.child(
                div()
                    .absolute()
                    .right(px(12.))
                    .bottom(px(12.))
                    .max_w(px(360.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .py(px(8.))
                    .pr(px(8.))
                    .pl(px(12.))
                    .bg(palette.panel)
                    .border_1()
                    .border_l_2()
                    .border_color(gpui::rgb(0xfc8397))
                    .rounded(px(6.))
                    .shadow_lg()
                    .child(div().flex_1().child(message))
                    .child(
                        crate::assets::icon_button("dismiss-error", "close", palette).on_click(
                            cx.listener(|root, _, _, cx| {
                                root.error = None;
                                cx.notify();
                            }),
                        ),
                    ),
            );
        }
        if self.session_menu.is_some() {
            content = content.child(self.render_session_menu(palette, window, cx));
        }
        if self.profile_menu.is_some() {
            content = content.child(self.render_profile_menu(palette, window, cx));
        }
        if self.dialog.is_some() {
            content = content.child(self.render_dialog(palette, cx));
        }
        self.finish_semantic_root(content, cx)
    }
}

impl Root {
    pub(crate) fn terminal_workspace(
        &self,
        window: &mut Window,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(feature = "experimental-shellfox-native")]
        if !self.is_local_source() {
            return self.peer_workspace(palette, window, cx);
        }
        let session = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .sessions
                .iter()
                .find(|session| Some(session.id) == self.selected_session())
        });
        let mut strip = div()
            .h(px(30.))
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(palette.sidebar)
            .border_b_1()
            .border_color(palette.border);
        if let Some(session) = session {
            let selected = self.selected_tab();
            let selected_index = selected.and_then(|id| {
                session
                    .tabs
                    .iter()
                    .position(|tab| tab.id == id)
                    .map(|index| (id, index))
            });
            let semantic_frame = self.semantic_context.frame();
            let tab_list_id = match semantic_frame.id_for("workspace/terminal-tabs") {
                Ok(id) => Some(id),
                Err(error) => {
                    semantic_frame.reject(error);
                    None
                }
            };
            let mut tabs = self.tab_scroll.viewport(session.id, selected_index);
            self.tab_scroll.retain_focus_handles(
                &session
                    .tabs
                    .iter()
                    .map(|tab| tab.id)
                    .collect::<HashSet<_>>(),
            );
            let reorder = self.tab_reorder_scope(cx);
            for tab in &session.tabs {
                let id = tab.id;
                let active = selected == Some(id);
                let reorder = reorder.filter(|_| tab.state != TabState::Archived);
                let title = gpui::SharedString::from(tab.title.clone());
                let focus = self.tab_scroll.focus_handle(id, cx);
                let click_focus = focus.clone();
                let mut label = button(format!("select-{id}"), title.clone(), palette)
                    .track_focus(&focus)
                    .on_click(cx.listener(move |root, _, window, cx| {
                        click_focus.focus(window);
                        root.select_tab(id, window, cx);
                    }));
                if let Some(scope) = reorder {
                    label = scope.drag_label(label, id, title.clone(), palette);
                    label = scope.keyboard_reorder(label, id, cx);
                }
                let label = if let Some(parent) = tab_list_id {
                    match semantic_frame.id_for(&format!("workspace/terminal-tab/{id}")) {
                        Ok(node_id) => {
                            let mut node = SemanticNode::new(
                                node_id,
                                Some(parent),
                                Role::Tab,
                                tab.title.clone(),
                            );
                            node.data.set_selected(active);
                            if reorder.is_some() {
                                node.data.set_description(
                                    "Alt+Left or Alt+Right moves this tab one position.",
                                );
                            }
                            node.focus = Some(focus);
                            semantic_frame.wrap(node, label).into_any_element()
                        }
                        Err(error) => {
                            semantic_frame.reject(error);
                            label.into_any_element()
                        }
                    }
                } else {
                    label.into_any_element()
                };
                let item = div()
                    .id(gpui::SharedString::from(format!("tab-{id}")))
                    .h_full()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .min_w(px(90.))
                    .max_w(px(180.))
                    .border_r_1()
                    .border_b_2()
                    .border_color(if active {
                        palette.accent
                    } else {
                        palette.border
                    })
                    .when(active, |style| style.bg(palette.panel))
                    .child(label)
                    .child(
                        button(format!("close-{id}"), "x", palette).on_click(cx.listener(
                            move |root, _, window, cx| {
                                root.modal_focus.focus(window);
                                root.dialog = Some(Dialog::CloseTab(id));
                                cx.notify();
                            },
                        )),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |root, _, window, cx| {
                            root.rename_tab_dialog(id, window, cx)
                        }),
                    )
                    .when_some(reorder, |item, scope| {
                        scope.drop_target(item, id, palette, cx)
                    });
                tabs = tabs.child(item);
            }
            let tabs = if let Some(id) = tab_list_id {
                semantic_frame
                    .wrap(
                        SemanticNode::new(
                            id,
                            Some(semantic_frame.root_id()),
                            Role::TabList,
                            "Terminals",
                        ),
                        tabs,
                    )
                    .into_any_element()
            } else {
                tabs.into_any_element()
            };
            let session_id = session.id;
            let new_tab = crate::assets::icon_button("new-tab", "plus", palette)
                .on_click(
                    cx.listener(move |root, _, window, cx| root.add_tab(session_id, window, cx)),
                )
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(|root, event: &gpui::MouseDownEvent, window, cx| {
                        root.open_profile_menu(event.position, window, cx);
                        cx.stop_propagation();
                    }),
                );
            strip = strip.child(tabs).child(new_tab);
        }
        strip = strip.child(div().flex_1()).child(
            crate::assets::icon_button("toggle-files", "folder", palette)
                .on_click(cx.listener(|root, _, _, cx| root.toggle_files(cx))),
        );
        let mut area = div().flex_1().min_h_0().min_w_0().flex().flex_col();
        if let Some(id) = self.selected_tab() {
            if let Some(view) = self.views.get(&id) {
                area = area.child(view.clone());
            } else if let Some(error) = self.failures.get(&id) {
                area = area.child(div().p_3().child(error.message.clone()));
            } else if let Some(status) = self.ssh_status(id) {
                area = area.child(div().p_3().text_color(palette.muted).child(status));
            } else {
                area = area.child(div().p_3().child(
                    button("start-terminal", "Open terminal", palette).on_click(
                        cx.listener(|root, _, window, cx| root.ensure_terminal(window, cx)),
                    ),
                ));
            }
        } else {
            area = area.child(self.empty_workspace(palette, window));
        }
        let terminal = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(strip)
            .child(area);
        let mut region = div().relative().flex().size_full().child(terminal);
        if self.files.as_ref().is_some_and(|files| files.visible) {
            if self.files_narrow(window) {
                region = region.child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(self.files_panel(palette, window, cx)),
                );
            } else {
                region = region.child(self.files_panel(palette, window, cx));
            }
        }
        region.into_any_element()
    }

    pub(super) fn handle_tab_semantic_action(
        &mut self,
        action: &crate::accessibility::SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(
            &action.kind,
            crate::accessibility::SemanticActionKind::Focus
                | crate::accessibility::SemanticActionKind::Click
        ) {
            return false;
        }
        let Some(session_id) = self.selected_session() else {
            return false;
        };
        let Some(tab_id) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| {
                    session.tabs.iter().find_map(|tab| {
                        let key = format!("workspace/terminal-tab/{}", tab.id);
                        (self.semantic_context.frame().id_for(&key).ok() == Some(action.target))
                            .then_some(tab.id)
                    })
                })
        }) else {
            return false;
        };
        let focus = self.tab_scroll.focus_handle(tab_id, cx);
        focus.focus(window);
        if matches!(
            &action.kind,
            crate::accessibility::SemanticActionKind::Click
        ) {
            self.select_tab(tab_id, window, cx);
        }
        cx.notify();
        true
    }
}
