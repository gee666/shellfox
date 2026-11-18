use crate::{
    chrome::button,
    root::{Dialog, Root},
    theme::Palette,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

impl Root {
    pub(crate) fn render_session_menu(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some((id, position)) = self.session_menu else {
            return div().into_any_element();
        };
        let Some(session) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
        else {
            return div().into_any_element();
        };
        let pinned = session.pinned_at.is_some();
        let settled = session.settled_at.is_some();
        let cwd = session.cwd.clone();
        let viewport = window.viewport_size();
        let x = f32::from(position.x).clamp(4., (f32::from(viewport.width) - 184.).max(4.));
        let y = f32::from(position.y).clamp(4., (f32::from(viewport.height) - 170.).max(4.));
        div()
            .id("session-context-menu")
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(180.))
            .p_1()
            .rounded(px(6.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|root, _, _, cx| {
                root.session_menu = None;
                cx.notify();
            }))
            .child(
                button(
                    "menu-pin",
                    if pinned { "Unpin" } else { "Pin to top" },
                    palette,
                )
                .w_full()
                .justify_start()
                .on_click(cx.listener(move |root, _, window, cx| {
                    root.session_menu = None;
                    root.toggle_session_pinned(id, !pinned, window, cx);
                })),
            )
            .child(
                button("menu-rename", "Rename", palette)
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(move |root, _, window, cx| {
                        root.session_menu = None;
                        root.rename_session_dialog(id, window, cx);
                    })),
            )
            .child(
                button("menu-copy-path", "Copy path", palette)
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(move |root, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(cwd.clone()));
                        root.session_menu = None;
                        cx.notify();
                    })),
            )
            .child(
                button(
                    "menu-settle",
                    if settled { "Unsettle" } else { "Settle" },
                    palette,
                )
                .w_full()
                .justify_start()
                .on_click(cx.listener(move |root, _, window, cx| {
                    root.session_menu = None;
                    root.toggle_session_settled(id, !settled, window, cx);
                })),
            )
            .child(
                button("menu-delete", "Delete…", palette)
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(move |root, _, window, cx| {
                        root.session_menu = None;
                        root.modal_focus.focus(window);
                        root.dialog = Some(Dialog::DeleteSession(id));
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
