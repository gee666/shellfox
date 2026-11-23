use super::{close::CloseButton, menu::TabMenu};
use crate::{
    accessibility::{Role, SemanticNode},
    assets::tooltip,
    dialogs::Dialog,
    root::Root,
    theme::Palette,
    workspace::tabs::activity::status_dot,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use shellfox_core::{Session, TabId};
use std::{collections::HashSet, rc::Rc};

mod labels;
use labels::tab_label_text;

impl Root {
    pub(super) fn render_terminal_tabs(
        &self,
        session: &Session,
        window: &Window,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected_tab();
        let visible_tabs: Vec<_> = session
            .tabs
            .iter()
            .filter(|tab| self.local_tab_viewable(tab))
            .collect();
        let visible: Rc<[TabId]> = visible_tabs.iter().map(|tab| tab.id).collect();
        let selected_index = selected.and_then(|id| {
            visible
                .iter()
                .position(|tab| *tab == id)
                .map(|index| (id, index))
        });
        let semantic_frame = self.semantics.context.frame();
        let tab_list_id = match semantic_frame.id_for("workspace/terminal-tabs") {
            Ok(id) => Some(id),
            Err(error) => {
                semantic_frame.reject(error);
                None
            }
        };
        let reorder = self.tab_reorder_scope(cx);
        let mut tabs =
            self.tabs
                .scroll
                .viewport(session.id, selected_index, Rc::clone(&visible), reorder);
        self.tabs.scroll.retain_focus_handles(
            &session
                .tabs
                .iter()
                .map(|tab| tab.id)
                .collect::<HashSet<_>>(),
            &visible
                .iter()
                .filter_map(|&tab| self.tab_close_identity(tab))
                .collect(),
        );
        let close_locked = self.dialogs.user_action_pending
            || matches!(self.dialogs.current, Some(Dialog::RenameTab { .. }));
        for (index, tab) in visible_tabs.iter().copied().enumerate() {
            let id = tab.id;
            let active = selected == Some(id);
            let state = self.tab_agent_state(tab, cx);
            let administrator =
                tab.profile_id.as_str() == shellfox_terminal::elevated::ADMINISTRATOR_PROFILE_ID;
            let title = gpui::SharedString::from(self.display_tab_title(tab).to_owned());
            let close_target = self.tab_close_identity(id);
            let (visible_title, title_width) =
                tab_label_text(window, title.clone(), administrator, close_target.is_some());
            let focus = self.tabs.scroll.focus_handle(id, cx).tab_stop(active);
            let click_focus = focus.clone();
            let context_focus = focus.clone();
            let mut label = div()
                .id(gpui::SharedString::from(format!("select-{id}")))
                .flex_1()
                .min_w_0()
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.))
                .text_color(if active {
                    palette.foreground
                } else {
                    palette.muted
                })
                .cursor_pointer()
                .hover(|style| style.bg(palette.row_hover))
                .track_focus(&focus)
                .on_mouse_down(gpui::MouseButton::Right, move |_, window, _| {
                    context_focus.focus(window)
                })
                .on_key_down(cx.listener({
                    let visible = Rc::clone(&visible);
                    move |root, event: &gpui::KeyDownEvent, window, cx| {
                        root.handle_tab_key(&visible, index, reorder, event, window, cx);
                    }
                }))
                .when(
                    self.tabs.menu.is_none() && self.tabs.profile_menu.is_none(),
                    |label| {
                        label.tooltip(tooltip(
                    format!("{title}. Double-click or F2 to rename. Drag or Alt+Arrow to reorder."),
                    palette,
                ))
                    },
                )
                .child(status_dot(state, palette))
                .child(
                    div()
                        .min_w_0()
                        .when_some(title_width, |label, width| label.w(width))
                        .truncate()
                        .when(cfg!(windows), |title| title.relative().top(px(1.)))
                        .child(visible_title),
                )
                .on_click(
                    cx.listener(move |root, event: &gpui::ClickEvent, window, cx| {
                        if event.click_count() > 1 && reorder.is_some() {
                            root.rename_tab_dialog(id, window, cx);
                        } else {
                            click_focus.focus(window);
                            root.select_tab(id, window, cx);
                        }
                    }),
                );
            if let Some(scope) = reorder {
                label = scope.drag_label(label, id, title.clone(), palette);
            }
            let label = if let Some(parent) = tab_list_id {
                match semantic_frame.id_for(&format!("workspace/terminal-tab/{id}")) {
                    Ok(node_id) => {
                        let name = if administrator {
                            format!("{title}, Administrator Windows PowerShell")
                        } else {
                            title.to_string()
                        };
                        let mut node = SemanticNode::new(node_id, Some(parent), Role::Tab, name);
                        node.data.set_selected(active);
                        node.data.set_description(self.tab_status_label(id, state));
                        if reorder.is_some() {
                            node.data.set_keyboard_shortcut("Alt+Left, Alt+Right");
                        }
                        node.focus = Some(focus.clone());
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
            let label = if let Some(Dialog::RenameTab {
                id: draft, title, ..
            }) = &self.dialogs.current
                && *draft == id
            {
                self.tab_title_editor(title.clone(), cx)
            } else {
                label
            };
            let show_close = active || focus.is_focused(window);
            let item = div()
                .id(gpui::SharedString::from(format!("tab-{id}")))
                .group("terminal-tab")
                .relative()
                .h_full()
                .flex()
                .flex_shrink_0()
                .items_start()
                .min_w(px(90.))
                .max_w(px(180.))
                .pr(px(1.))
                .child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .w(px(1.))
                        .bg(palette.border),
                )
                .when(active, |style| style.bg(palette.panel))
                .when(administrator, |item| {
                    item.child(
                        div()
                            .id(("administrator-tab", index))
                            .flex_none()
                            .ml(px(5.))
                            .mt(px(6.))
                            .px(px(3.))
                            .rounded(px(2.))
                            .border_1()
                            .border_color(palette.muted)
                            .text_size(px(10.))
                            .line_height(px(14.))
                            .text_color(palette.foreground)
                            .font_family(if cfg!(windows) {
                                "Consolas"
                            } else {
                                "monospace"
                            })
                            .tooltip(tooltip("Administrator Windows PowerShell", palette))
                            .child("Admin"),
                    )
                })
                .child(label)
                .when_some(close_target, |item, target| {
                    item.child(self.tab_close_button(
                        CloseButton {
                            target,
                            visible: show_close,
                            locked: close_locked,
                        },
                        &title,
                        palette,
                        cx,
                    ))
                })
                .when(active, |style| {
                    style.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(2.))
                            .bg(palette.accent),
                    )
                })
                .when_some(close_target.filter(|_| !close_locked), |item, target| {
                    item.on_mouse_up(
                        gpui::MouseButton::Middle,
                        cx.listener(move |root, _, window, cx| {
                            root.request_tab_close(target, window, cx);
                        }),
                    )
                })
                .when_some(reorder, |item, scope| {
                    let item = item.on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |root, event: &gpui::MouseDownEvent, window, cx| {
                            root.open_tab_menu(TabMenu::new(scope, id, event.position), window, cx);
                        }),
                    );
                    scope.drop_target(item, id, palette, cx)
                });
            tabs = tabs.child(item);
        }
        if let Some(id) = tab_list_id {
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
        }
    }

    pub(crate) fn focus_tab_label(&self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.scroll.reveal_on_paint(id);
        self.tabs.scroll.focus_handle(id, cx).focus(window);
        cx.notify();
    }

    pub(crate) fn handle_tab_semantic_action(
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
                        (self.semantics.context.frame().id_for(&key).ok() == Some(action.target))
                            .then_some(tab.id)
                    })
                })
        }) else {
            return false;
        };
        self.focus_tab_label(tab_id, window, cx);
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
