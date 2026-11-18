use crate::{root::Root, theme::Palette, workspace::AfterAction};
use gpui::{Context, Div, EntityId, Render, SharedString, Stateful, Window, div, prelude::*, px};
use shellfox_core::TabState;
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
struct TabDrag {
    strip: TabStrip,
    tab: TabId,
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

    pub(super) fn keyboard_reorder(
        self,
        label: Stateful<Div>,
        tab: TabId,
        cx: &Context<Root>,
    ) -> Stateful<Div> {
        label.on_key_down(
            cx.listener(move |root, event: &gpui::KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.alt {
                    return;
                }
                let offset = match event.keystroke.key.as_str() {
                    "left" => -1,
                    "right" => 1,
                    _ => return,
                };
                window.prevent_default();
                cx.stop_propagation();
                root.move_local_tab(self, tab, offset, window, cx);
            }),
        )
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
        .drag_over::<TabDrag>(move |style, _, _, _| style.bg(palette.hover))
        .on_drop(cx.listener(move |root, drag: &TabDrag, window, cx| {
            root.drop_local_tab(*drag, tab, window, cx);
        }))
    }
}

impl Root {
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

    fn move_local_tab(
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
            .map(|tab| (tab.id, tab.state != TabState::Archived));
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
