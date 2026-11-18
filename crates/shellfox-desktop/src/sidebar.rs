use crate::{
    assets::{icon, icon_button},
    chrome::button,
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, div, prelude::*, px};
use shellfox_host::{Tab, TabState};

fn needs_fresh_terminal(tabs: &[Tab]) -> bool {
    tabs.iter().all(|tab| tab.state == TabState::Archived)
}

#[path = "sidebar_width.rs"]
mod width;
pub(crate) use width::clamp_width;

impl Root {
    pub(crate) fn session_sidebar(
        &self,
        palette: Palette,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = div()
            .id("sessions")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .py(px(4.));
        if let Some(snapshot) = &self.snapshot {
            list =
                list.child(
                    div()
                        .id("local-section")
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pl(px(9.))
                        .pr(px(4.))
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .cursor_pointer()
                        .child(div().w(px(12.)).flex_shrink_0().child(icon(
                            if self.local_expanded {
                                "down"
                            } else {
                                "chevron"
                            },
                        )))
                        .child(format!("Local · {}", snapshot.sessions.len()))
                        .on_click(cx.listener(|root, _, _window, cx| {
                            #[cfg(feature = "experimental-shellfox-native")]
                            if !root.is_local_source() {
                                root.select_local_source(_window, cx);
                                root.local_expanded = true;
                                cx.notify();
                                return;
                            }
                            root.local_expanded = !root.local_expanded;
                            cx.notify();
                        })),
                );
            if self.local_expanded {
                let shown = self.session_pages.saturating_mul(self.history_page_size);
                for session in snapshot.sessions.iter().take(shown) {
                    let id = session.id;
                    let selected = self.selected_session() == Some(id);
                    let failed = session.tabs.iter().any(|tab| {
                        self.views
                            .get(&tab.id)
                            .is_some_and(|view| view.read(cx).is_failed())
                            || self.failures.contains_key(&tab.id)
                    });
                    let active = session.tabs.iter().any(|tab| {
                        self.views.get(&tab.id).is_some_and(|view| {
                            view.read(cx).runtime_phase() == Some(shellfox_runtime::Phase::Active)
                        })
                    });
                    let selected_background = palette.accent.opacity(0.09);
                    list = list.child(
                        div()
                            .id(gpui::SharedString::from(format!("session-{id}")))
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
                                palette.sidebar
                            })
                            .when(selected, |style| style.bg(selected_background))
                            .cursor_pointer()
                            .hover(move |style| {
                                style.bg(if selected {
                                    selected_background
                                } else {
                                    palette.row_hover
                                })
                            })
                            .child(
                                div()
                                    .mt(px(3.))
                                    .w(px(8.))
                                    .h(px(8.))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(if failed {
                                        gpui::rgb(0xfc8397).into()
                                    } else if session.settled_at.is_some() {
                                        gpui::rgb(0x96734e).into()
                                    } else if active {
                                        gpui::rgb(0x60a5fa).into()
                                    } else {
                                        palette.muted
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .line_height(px(14.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child(div().truncate().child(session.title.clone()))
                                            .when(session.pinned_at.is_some(), |style| {
                                                style.child(icon("pin").size(px(11.)))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(palette.muted)
                                            .truncate()
                                            .child(session.cwd.clone()),
                                    ),
                            )
                            .on_click(cx.listener(
                                move |root, event: &gpui::ClickEvent, window, cx| {
                                    if event.click_count() > 1 {
                                        root.rename_session_dialog(id, window, cx);
                                    } else {
                                        root.select_session(id, window, cx);
                                    }
                                },
                            ))
                            .on_mouse_down(
                                gpui::MouseButton::Right,
                                cx.listener(move |root, event: &gpui::MouseDownEvent, _, cx| {
                                    root.session_menu = Some((id, event.position));
                                    cx.notify();
                                }),
                            ),
                    );
                    if selected && !self.settings && needs_fresh_terminal(&session.tabs) {
                        // Imported sessions retain their folder, not their old shell.
                        let ready = self.host.is_some()
                            && !self.action_pending
                            && !self.closing_window
                            && self.dialog.is_none();
                        let open = button(
                            format!("open-saved-session-{id}"),
                            "Open terminal",
                            palette,
                        )
                        .when(!ready, |button| button.opacity(0.5))
                        .when(ready, |button| {
                            button.on_click(cx.listener(move |root, _, window, cx| {
                                if !root.settings
                                    && root.dialog.is_none()
                                    && root.selected_session() == Some(id)
                                    && root.snapshot.as_ref().is_some_and(|snapshot| {
                                        snapshot.sessions.iter().any(|session| {
                                            session.id == id && needs_fresh_terminal(&session.tabs)
                                        })
                                    })
                                {
                                    root.add_tab(id, window, cx);
                                }
                            }))
                        });
                        list = list.child(div().pl(px(20.)).flex().child(open));
                    }
                }
                if shown < snapshot.sessions.len() {
                    list = list.child(
                        div().flex().mx(px(12.)).my(px(4.)).child(
                            div()
                                .id("more-sessions")
                                .px(px(4.))
                                .py(px(4.))
                                .text_size(px(12.))
                                .text_color(palette.muted)
                                .cursor_pointer()
                                .hover(|style| style.text_color(palette.accent))
                                .child("Show more")
                                .on_click(cx.listener(|root, _, _, cx| {
                                    root.session_pages = root.session_pages.saturating_add(1);
                                    cx.notify();
                                })),
                        ),
                    );
                }
                if snapshot.sessions.is_empty() {
                    list = list.child(
                        div()
                            .mx(px(8.))
                            .my(px(12.))
                            .text_size(px(12.))
                            .text_color(palette.muted)
                            .child("No sessions, press +"),
                    );
                }
            }
        }
        #[cfg(feature = "experimental-shellfox-native")]
        {
            list = self.peer_sidebar(list, palette, cx);
        }
        div()
            .w(px(clamp_width(
                self.sidebar_width,
                f32::from(window.viewport_size().width),
            )))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.sidebar)
            .child(
                div()
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(2.))
                    .gap(px(4.))
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        icon_button("hide-sessions", "back", palette)
                            .w(px(24.))
                            .on_click(cx.listener(|root, _, _, cx| {
                                root.sidebar = false;
                                root.resizing_sidebar = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon_button("new-session", "plus", palette).on_click(
                                cx.listener(|root, _, window, cx| root.create_dialog(window, cx)),
                            ))
                            .child(self.render_power_button(palette, cx))
                            .child(
                                icon_button("settings", "settings", palette)
                                    .on_click(cx.listener(|root, _, _, cx| root.open_settings(cx))),
                            ),
                    ),
            )
            .child(list)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_host::{ProfileId, SessionId, TabId};

    #[test]
    fn offers_a_fresh_terminal_only_when_no_saved_tab_can_be_selected() {
        let archived = Tab {
            id: TabId::new(),
            session_id: SessionId::new(),
            title: "Saved shell".into(),
            cwd: if cfg!(windows) { "C:/project" } else { "/project" }.into(),
            ordinal: 0,
            created_at: None,
            profile_id: ProfileId::new("login-shell").unwrap(),
            state: TabState::Archived,
        };
        let pending = Tab {
            state: TabState::Pending,
            ..archived.clone()
        };
        assert!(needs_fresh_terminal(&[]));
        assert!(needs_fresh_terminal(std::slice::from_ref(&archived)));
        assert!(!needs_fresh_terminal(&[archived, pending]));
    }
}
