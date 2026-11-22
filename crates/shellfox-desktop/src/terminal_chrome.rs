use super::tab_scroll::{TabFocus, TabScroll};
use crate::{
    assets::{icon_button, tooltip},
    chrome::button,
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

impl Root {
    pub(crate) fn terminal_workspace(
        &self,
        window: &mut Window,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.is_local_source() {
            return self.peer_workspace(palette, window, cx);
        }
        let session = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .sessions
                .iter()
                .find(|session| Some(session.id) == self.selected_session())
        });
        let windowed = session.is_some_and(|session| {
            TabScroll::windowed(
                session
                    .tabs
                    .iter()
                    .filter(|tab| self.local_tab_viewable(tab))
                    .count(),
            )
        });
        let mut strip = div()
            .h(px(30.))
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .items_center()
            .on_key_down(cx.listener(Self::tab_traversal))
            .bg(palette.sidebar)
            .border_b_1()
            .border_color(palette.border);
        if let Some(session) = session {
            strip = strip.child(self.render_terminal_tabs(session, window, palette, cx));
            let accepts_tabs = self.host.is_some()
                && !self.user_action_pending
                && !session.tabs.iter().any(|tab| tab.profile_id.as_str() == shellfox_service::AUTOMATION_PROFILE_ID);
            let can_open_menu = accepts_tabs && !self.profiles.is_empty();
            let can_add = accepts_tabs && self.profiles.iter().any(|profile| profile.available);
            let key = TabFocus::NewTerminal;
            let new_tab = icon_button("new-tab", "plus", palette)
                .mx(px(4.))
                .tooltip(tooltip("New terminal. Right-click to choose a shell.", palette))
                .when(!can_add, |button| button.opacity(0.4).cursor(gpui::CursorStyle::Arrow))
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |root, event: &gpui::MouseDownEvent, window, cx| {
                        if can_open_menu { root.open_profile_menu(event.position, window, cx); }
                        cx.stop_propagation();
                    }),
                );
            let focus = can_add.then(|| self.tab_scroll.control_focus(key, cx).tab_stop(true));
            strip = strip.child(self.tab_strip_control(key, "New terminal", new_tab, focus, palette, cx));
        }
        let files_available = matches!(
            cx.try_global::<crate::editor_composition::EditorOutcome>(),
            Some(crate::editor_composition::EditorOutcome::Ready(_))
        );
        if files_available || session.is_none() {
            strip = strip.when(!windowed, |strip| strip.child(div().flex_1()))
                .child(self.render_files_toggle(files_available, palette, cx));
        }
        if files_available && session.is_some() {
            let folder_available = self.selected_tab().is_some_and(|id| {
                self.views.get(&id).is_some_and(|view| {
                    view.read(cx).runtime_phase() == Some(shellfox_runtime::Phase::Active)
                })
            }) && !self.files.as_ref().is_some_and(|files| files.busy);
            let key = TabFocus::Folder;
            let button = icon_button("open-terminal-folder", "folder-terminal", palette)
                .tooltip(tooltip("Open terminal folder", palette))
                .when(!folder_available, |button| button.opacity(0.4));
            let focus = folder_available.then(|| self.tab_scroll.control_focus(key, cx).tab_stop(true));
            strip = strip.child(self.tab_strip_control(key, "Open terminal folder", button, focus, palette, cx));
        }
        let mut area = div().relative().flex_1().min_h_0().min_w_0().flex().flex_col();
        if let Some(id) = self.selected_tab() {
            if let Some(view) = self.views.get(&id) {
                area = area.child(view.clone());
            } else if let Some(error) = self.failures.get(&id) {
                if error.control_required {
                    let taking = self.attaching.contains(&id);
                    area = area.child(div().size_full().flex().flex_col().items_center().justify_center()
                        .gap(px(8.)).text_size(px(12.)).text_color(palette.muted)
                        .child("Another client is controlling this terminal")
                        .child(button("take-unattached-terminal", "Take it", palette)
                            .bg(palette.surface).text_color(palette.foreground)
                            .border_1().border_color(palette.border).rounded(px(4.))
                            .when(taking, |button| button.opacity(0.5))
                            .when(!taking, |button| button.on_click(cx.listener(move |root, _, window, cx| {
                                root.attach_local_view(id, Some(crate::app::TerminalRecovery::TakeControl), window, cx);
                            })))));
                } else {
                    area = area.child(div().absolute().right(px(12.)).top(px(8.)).child(
                        button("retry-unattached-terminal", "Reconnect terminal", palette)
                            .on_click(cx.listener(|root, _, window, cx| root.ensure_terminal(window, cx)))
                    )).child(div().absolute().right(px(12.)).bottom(px(8.))
                        .text_size(px(11.)).text_color(palette.muted).child(error.message.clone()));
                }
            } else if self.attaching.contains(&id) {
                area = area.child(crate::renderer::terminal_loading(palette.accent));
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
            .relative()
            .child(strip)
            .child(area)
            .when(
                matches!(self.dialog, Some(crate::root::Dialog::CloseTab(_))),
                |workspace| workspace.child(self.render_tab_close_confirmation(palette, cx)),
            );
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
}
