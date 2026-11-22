use super::tab_menu::TabMenu;
use crate::{root::Root, theme::Palette, workspace::AfterAction};
use gpui::{Context, Div, EntityId, Render, SharedString, Stateful, Window, div, prelude::*, px};
use shellfox_host::{HostRequest, SessionId, TabId};
use shellfox_service::native::NativeAction;

#[path = "tab_order.rs"]
mod order;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct TabStrip {
    window_root: EntityId,
    session: SessionId,
    selection_epoch: u64,
    source_epoch: u64,
}

#[derive(Clone, Copy)]
pub(super) struct TabDrag {
    pub(super) strip: TabStrip,
    pub(super) tab: TabId,
}

struct TabPreview {
    title: SharedString,
    palette: Palette,
}

impl Render for TabPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .max_w(px(180.))
            .rounded_sm()
            .bg(self.palette.panel)
            .text_color(self.palette.foreground)
            .text_size(px(12.))
            .truncate()
            .child(self.title.clone())
    }
}

impl TabStrip {
    pub(super) fn drag_label(
        self,
        label: Stateful<Div>,
        tab: TabId,
        title: SharedString,
        palette: Palette,
    ) -> Stateful<Div> {
        label.on_drag(TabDrag { strip: self, tab }, move |_, _, _, cx| {
            cx.new(|_| TabPreview {
                title: title.clone(),
                palette,
            })
        })
    }

    pub(super) fn drop_target(
        self,
        item: Stateful<Div>,
        tab: TabId,
        palette: Palette,
        cx: &Context<Root>,
    ) -> Stateful<Div> {
        item.can_drop(move |value, _, _| {
            value
                .downcast_ref::<TabDrag>()
                .is_some_and(|drag| drag.strip == self && drag.tab != tab)
        })
        .drag_over::<TabDrag>(move |style, drag, _, _| {
            if drag.strip == self && drag.tab != tab {
                style.border_l_2().border_color(palette.accent)
            } else {
                style
            }
        })
        .on_drop(cx.listener(move |root, drag: &TabDrag, window, cx| {
            root.drop_local_tab(*drag, tab, window, cx);
        }))
    }
}

impl Root {
    pub(super) fn handle_tab_key(
        &mut self,
        tabs: &[TabId],
        index: usize,
        reorder: Option<TabStrip>,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        let id = tabs[index];
        match key {
            "f2" if reorder.is_some() => self.rename_tab_dialog(id, window, cx),
            "contextmenu" | "menu" | "f10" if key != "f10" || mods.shift => {
                if let Some(scope) = reorder
                    && let Some(bounds) = self.tab_scroll.item_bounds(index)
                {
                    self.open_tab_menu(
                        TabMenu::new(
                            scope,
                            id,
                            gpui::point(bounds.left(), bounds.bottom() - px(2.)),
                        )
                        .with_keyboard_focus(),
                        window,
                        cx,
                    );
                }
            }
            "left" | "right" if mods.alt => {
                if let Some(scope) = reorder {
                    self.move_local_tab(scope, id, if key == "left" { -1 } else { 1 }, window, cx);
                }
            }
            "enter" | "space" => self.select_tab(id, window, cx),
            _ => {
                let target = match key {
                    "left" => (index + tabs.len() - 1) % tabs.len(),
                    "right" => (index + 1) % tabs.len(),
                    "home" => 0,
                    "end" => tabs.len() - 1,
                    _ => return,
                };
                let id = tabs[target];
                // Keep the label focused while a new view opens, then let the
                // normal selection path hand focus to the terminal.
                self.focus_tab_label(id, window, cx);
                self.select_tab(id, window, cx);
            }
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    pub(super) fn tab_reorder_scope(&self, cx: &Context<Self>) -> Option<TabStrip> {
        if self.host.is_none()
            || self.closing_window
            || self.user_action_pending
            || self.settings
            || self.dialog.is_some()
            || self.session_menu.is_some()
            || self.profile_menu.is_some()
        {
            return None;
        }
        Some(TabStrip {
            window_root: cx.entity_id(),
            session: self.selected_session()?,
            selection_epoch: self.navigation.epoch(),
            source_epoch: self.source_selection_epoch(),
        })
    }

    fn drop_local_tab(
        &mut self,
        drag: TabDrag,
        target: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reorder_local_tab(drag.strip, drag.tab, Some(target), None, window, cx);
    }

    pub(super) fn can_move_local_tab(&self, strip: TabStrip, id: TabId, offset: isize) -> bool {
        let Some(session) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .sessions
                .iter()
                .find(|session| session.id == strip.session)
        }) else {
            return false;
        };
        let tabs = || {
            session
                .tabs
                .iter()
                .filter(|tab| self.local_tab_viewable(tab))
        };
        let Some(index) = tabs().position(|tab| tab.id == id) else {
            return false;
        };
        index
            .checked_add_signed(offset)
            .is_some_and(|target| target < tabs().count())
    }

    pub(super) fn move_local_tab(
        &mut self,
        strip: TabStrip,
        source: TabId,
        offset: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reorder_local_tab(strip, source, None, Some(offset), window, cx);
    }

    fn reorder_local_tab(
        &mut self,
        strip: TabStrip,
        source: TabId,
        target: Option<TabId>,
        offset: Option<isize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_reorder_scope(cx) != Some(strip) {
            return;
        }
        let Some(session) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .sessions
                .iter()
                .find(|session| session.id == strip.session)
        }) else {
            return;
        };
        let tabs = session
            .tabs
            .iter()
            .map(|tab| (tab.id, self.local_tab_viewable(tab)));
        let tab_ids = match (target, offset) {
            (Some(target), None) => order::move_tab(tabs, source, target),
            (None, Some(offset)) => order::move_tab_by(tabs, source, offset),
            _ => None,
        };
        let Some(tab_ids) = tab_ids else {
            return;
        };
        self.submit_native(
            NativeAction::Metadata(HostRequest::ReorderTabs {
                session_id: strip.session,
                tab_ids,
            }),
            AfterAction::Mutation,
            window,
            cx,
        );
    }
}
