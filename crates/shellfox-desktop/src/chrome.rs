use crate::{root::Root, theme::Palette};
use gpui::{Context, Div, Render, Window, div, prelude::*, px};

mod notifications;
pub(crate) use notifications::Notifications;

#[cfg(target_os = "linux")]
mod linux_chrome;
mod tab_close;
mod tab_reorder;
mod tab_scroll;
mod tab_focus;
mod tab_menu;
mod terminal_chrome;
mod terminal_tabs;
pub(crate) use tab_menu::TabMenu;
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
        self.sync_file_source(cx);
        cx.defer_in(window, |root, window, cx| root.after_render(window, cx));
        self.sidebar_ui.list.begin_frame();
        self.html_preview.begin_frame();
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
            .or_else(|| cx.try_global::<crate::startup_chrome::StartupPalette>().map(|colors| colors.palette))
            .unwrap_or_else(|| Palette::new("#ec4899", "#111016"));
        self.sync_field_palette(palette, cx);
        self.sync_dialog_edit_state(cx);
        self.sync_semantic_children(cx);
        if let Some(fixture) = &self.fixture {
            return self.finish_semantic_root(fixture.clone(), cx);
        }
        self.ensure_selected_views(window, cx);
        let mut content = div()
            .relative()
            .track_focus(&self.layout_focus)
            .tab_stop(false)
            .flex()
            .size_full()
            .overflow_hidden()
            .bg(palette.background)
            .text_color(palette.foreground)
            .font_family(crate::ui_font::family())
            .text_size(px(13.))
            .key_context(self.layout_key_context(cx))
            .on_action(cx.listener(Self::on_toggle_sessions))
            .on_action(cx.listener(Self::on_toggle_files))
            .on_action(cx.listener(Self::on_toggle_folders))
            .child(motion);
        let body = if self.files.as_ref().is_some_and(|files| files.visible && files.maximized) {
            self.files_panel(palette, window, cx)
        } else {
            self.layout_body(palette, window, cx)
        };
        #[cfg(target_os = "linux")]
        let body = match Self::linux_titlebar(palette, window, cx) {
            Some(title) => div().size_full().flex().flex_col()
                .child(title)
                .child(div().flex_1().min_h_0().w_full().child(body))
                .into_any_element(),
            None => body,
        };
        content = content.child(body);
        if self.closing_window && !matches!(self.dialog, Some(crate::root::Dialog::Quit)) {
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
        let new_session_menu = self.render_new_session_menu(palette, window, cx);
        self.html_preview.end_frame(
            self.closing_window || self.settings || self.dialog.is_some() || new_session_menu.is_some()
                || self.session_menu.is_some() || self.profile_menu.is_some() || self.tab_menu.is_some()
                || self.files.as_ref().is_some_and(|files| files.pending_file_action.is_some()),
        );
        content = content.children(new_session_menu);
        if self.session_menu.is_some() {
            content = content.child(self.render_session_menu(palette, window, cx));
        }
        if self.profile_menu.is_some() {
            content = content.child(self.render_profile_menu(palette, window, cx));
        }
        if self.tab_menu.is_some() {
            content = content.child(self.render_tab_menu(palette, window, cx));
        }
        if self.dialog.is_some()
            && !matches!(self.dialog, Some(crate::root::Dialog::RenameTab { .. } | crate::root::Dialog::CloseTab(_)))
        {
            content = content.child(self.render_dialog(palette, window, cx));
        }
        self.collect_errors(cx);
        content = content.child(self.render_toasts(palette, window, cx));
        let weak = cx.weak_entity();
        content = content.child(gpui::canvas(|_, _, _| (), move |_, (), window, cx| {
            window.defer(cx, move |window, cx| {
                let _ = weak.update(cx, |root, cx| root.poll_session_visibility(window, cx));
            });
        }).absolute().inset_0());
        self.finish_semantic_root(content, cx)
    }
}
