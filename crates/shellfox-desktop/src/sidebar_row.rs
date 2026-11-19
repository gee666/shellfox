use super::*;
use crate::{
    assets::{ellipsis, inline_ellipsis},
    root::Dialog,
    tab_activity::status_dot,
};
use gpui::{KeyDownEvent, MouseButton, Window};

pub(crate) fn shorten_home(cwd: &str) -> String {
    if let Some(rest) = cwd
        .strip_prefix(r"\\")
        .and_then(|path| path.split_once('\\'))
        && (rest.0.eq_ignore_ascii_case("wsl.localhost") || rest.0.eq_ignore_ascii_case("wsl$"))
        && !rest.1.is_empty()
    {
        let (distro, guest) = rest.1.split_once('\\').unwrap_or((rest.1, ""));
        if !distro.is_empty() && !distro.contains('/') {
            return shorten_home(&format!("/{}", guest.replace('\\', "/")));
        }
    }
    let bytes = cwd.as_bytes();
    if let [drive, b':', b'\\' | b'/', ..] = bytes
        && drive.is_ascii_alphabetic()
        && cwd
            .get(3..8)
            .is_some_and(|part| part.eq_ignore_ascii_case("users"))
        && matches!(bytes.get(8), Some(b'\\' | b'/'))
        && let Some(tail) = short_home_tail(&cwd[9..], ['/', '\\'])
    {
        return format!("~{tail}");
    }
    if let Some(tail) = cwd
        .strip_prefix("/home/")
        .or_else(|| cwd.strip_prefix("/Users/"))
        && let Some(tail) = short_home_tail(tail, ['/'])
    {
        return format!("~{tail}");
    }
    cwd.to_owned()
}

fn short_home_tail<const N: usize>(path: &str, separators: [char; N]) -> Option<&str> {
    let offset = path.find(separators).unwrap_or(path.len());
    (offset != 0).then(|| &path[offset..])
}

pub(crate) fn status_label(dot: AgentState, error: Option<&str>) -> String {
    match dot {
        AgentState::Error => {
            return error.map_or_else(|| "Error".into(), |message| format!("Error: {message}"));
        }
        AgentState::Busy => "Agent working",
        AgentState::Running => "Agent idle",
        AgentState::Shell => "Shell",
        AgentState::Settled => "Settled",
    }
    .into()
}

pub(crate) fn focus_ring(palette: Palette) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .border_1()
        .border_color(palette.accent)
}

impl Root {
    pub(super) fn session_row(
        &self,
        session: &Session,
        dot: AgentState,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = session.id;
        let Some((focus, settle_focus)) = self.sidebar_ui.row_focus.get(&id) else {
            return div().into_any_element();
        };
        let menu_focus = focus.clone();
        let selected = self.selected_session() == Some(id);
        let settled = dot == AgentState::Settled;
        let selected_bg = palette.accent.opacity(0.09);
        let mut copy = div()
            .id("session-select")
            .relative()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .line_height(px(14.));
        match &self.dialog {
            Some(Dialog::RenameSession { id: editing, title }) if *editing == id => {
                copy = copy.child(title.clone());
            }
            _ => {
                copy = copy
                    .track_focus(focus)
                    .cursor_pointer()
                    .when(
                        self.sidebar_ui.keyboard_focus && focus.is_focused(window),
                        |copy| copy.child(focus_ring(palette)),
                    )
                    .on_click(
                        cx.listener(move |root, event: &gpui::ClickEvent, window, cx| {
                            if event.click_count() <= 1 {
                                root.select_session(id, window, cx);
                            }
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .min_w_0()
                            .child(
                                div()
                                    .id("session-title")
                                    .min_w_0()
                                    .text_size(px(13.))
                                    .when(settled, |style| {
                                        style.text_color(palette.status_color(0x96734e))
                                    })
                                    .tooltip(tooltip(session.title.clone(), palette))
                                    .child(inline_ellipsis(session.title.clone(), 14.))
                                    .on_click(cx.listener(
                                        move |root, event: &gpui::ClickEvent, window, cx| {
                                            if event.click_count() > 1 {
                                                cx.stop_propagation();
                                                root.rename_session_dialog(id, window, cx);
                                            }
                                        },
                                    )),
                            )
                            .when(session.pinned_at.is_some(), |line| {
                                line.child(
                                    div()
                                        .id("session-pin")
                                        .flex_shrink_0()
                                        .text_color(palette.muted)
                                        .tooltip(tooltip("Pinned", palette))
                                        .child(icon_sized("pin", 11.)),
                                )
                            }),
                    );
            }
        }
        copy = copy.child(
            div()
                .id("session-path")
                .w_full()
                .min_w_0()
                .text_size(px(11.))
                .text_color(palette.muted)
                .tooltip(tooltip(session.cwd.clone(), palette))
                .child(ellipsis(shorten_home(&session.cwd), 14.)),
        );
        let settle = icon_button(
            "session-settle",
            if settled { "unsettle" } else { "settle" },
            palette,
        )
        .when(!self.user_action_pending, |button| {
            button.track_focus(settle_focus)
        })
        .w(px(20.))
        .h(px(24.))
        .focus(|style| style.border_1().border_color(palette.accent))
        .tooltip(tooltip(
            if settled { "Unsettle" } else { "Settle" },
            palette,
        ))
        .when(self.user_action_pending, |button| button.opacity(0.4))
        .on_click(cx.listener(move |root, _, window, cx| {
            if !root.user_action_pending {
                root.toggle_session_settled(id, !settled, window, cx);
            }
        }));
        div()
            .id(gpui::SharedString::from(format!("session-{id}")))
            .relative()
            .h(px(36.))
            .flex()
            .items_start()
            .gap(px(8.))
            .px(px(8.))
            .py(px(4.))
            .border_l_2()
            .border_color(if selected {
                palette.accent
            } else {
                gpui::transparent_black()
            })
            .when(selected, |style| style.bg(selected_bg))
            .hover(move |style| {
                style.bg(if selected {
                    selected_bg
                } else {
                    palette.row_hover
                })
            })
            .child(self.semantic_region(
                &format!("sidebar/local/{id}/status"),
                crate::accessibility::Role::Image,
                status_label(dot, self.session_error_text(session).as_deref()),
                status_dot(dot, palette).mt(px(3.)),
            ))
            .child(self.sidebar_ui.visibility.marker(crate::session_visibility::PollingTarget::Local(id)))
            .child(copy)
            .child(settle)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |root, event: &gpui::MouseDownEvent, window, cx| {
                    root.sidebar_ui.keyboard_focus = false;
                    if !matches!(root.dialog, Some(Dialog::RenameSession { id: editing, .. }) if editing == id) { menu_focus.focus(window); }
                    root.open_session_menu(id, event.position, false, window, cx);
                }),
            )
            .on_key_down(cx.listener(move |root, event: &KeyDownEvent, window, cx| {
                root.session_row_key(id, event, window, cx)
            }))
            .into_any_element()
    }

    fn session_row_key(
        &mut self,
        id: SessionId,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        if matches!(key, "contextmenu" | "context-menu" | "menu") || key == "f10" && mods.shift {
            let rows = self.sidebar_rows(cx);
            if let Some(index) = rows.iter().position(|(session, _)| session.id == id) {
                let bounds = super::windowed::bounds(&self.sidebar_ui.scroll, index);
                self.open_session_menu(
                    id,
                    gpui::point(bounds.left(), bounds.bottom()),
                    true,
                    window,
                    cx,
                );
            }
        } else if key == "tab" && !mods.control && !mods.alt && !mods.platform {
            if matches!(&self.dialog, Some(Dialog::RenameSession { id: editing, title }) if *editing == id && title.read(cx).is_composing())
            {
                return;
            }
            self.sidebar_focus_tab(id, mods.shift, window, cx);
            cx.notify();
        } else if let Some(Dialog::RenameSession { id: editing, title }) = &self.dialog
            && *editing == id
            && title.read(cx).edit_focus().is_focused(window)
        {
            if title.read(cx).is_composing() {
                return;
            }
            match key {
                "enter" => self.confirm_dialog(window, cx),
                "escape" if !self.user_action_pending => {
                    self.dialog = None;
                    self.sidebar_ui.focus_row(id, window);
                    cx.notify();
                }
                _ => return,
            }
        } else if mods.control || mods.alt || mods.platform {
            return;
        } else if matches!(key, "home" | "end" | "up" | "down") {
            let rows = self.sidebar_rows(cx);
            let len = rows
                .len()
                .min(self.session_pages.saturating_mul(self.history_page_size));
            let Some(index) = rows.iter().position(|(session, _)| session.id == id) else {
                return;
            };
            let next = match key {
                "home" => 0,
                "end" => len.saturating_sub(1),
                "up" if index > 0 => index - 1,
                "down" if index + 1 < len => index + 1,
                _ => return,
            };
            if let Some((target, _)) = rows.get(next) {
                self.sidebar_ui.focus_row(target.id, window);
                super::windowed::reveal(&self.sidebar_ui.scroll, next);
                cx.notify();
            }
        } else {
            return;
        }
        window.prevent_default();
        cx.stop_propagation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_shorten_home_in_windows_posix_and_wsl() {
        assert_eq!(shorten_home(r"C:\Users\me\project"), r"~\project");
        assert_eq!(shorten_home("c:/USERS/équipe"), "~");
        assert_eq!(shorten_home("/home/me/project"), "~/project");
        assert_eq!(shorten_home("/Users/me"), "~");
        assert_eq!(shorten_home(r"\\wsl.localhost\Ubuntu\home\me\app"), "~/app");
        assert_eq!(shorten_home(r"\\wsl$\Ubuntu"), "/");
        assert_eq!(shorten_home("/tmp/Users/me"), "/tmp/Users/me");
        assert_eq!(shorten_home("/home//work"), "/home//work");
    }
}
