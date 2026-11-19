use crate::{assets::tooltip, root::Root, theme::Palette, workspace::AfterAction};
#[path = "profile_menu_semantics.rs"]
mod semantics;
use gpui::{
    AnyElement, Context, FocusHandle, Pixels, Point, TextStyle, Window, div, prelude::*, px,
};
use shellfox_core::{AddTab, ProfileId, SessionId};
use shellfox_host::HostRequest;
use shellfox_service::native::NativeAction;

pub(crate) struct ProfileMenu {
    position: Point<Pixels>,
    session: SessionId,
    epoch: u64,
    focus: FocusHandle,
    choices: Vec<Choice>,
    previous_focus: Option<FocusHandle>,
    keyboard_selection: Option<usize>,
}
struct Choice {
    label: String,
    id: String,
    available: bool,
    reason: Option<String>,
    focus: FocusHandle,
}

impl Choice {
    fn hint(&self) -> &str {
        self.reason.as_deref().unwrap_or(if !self.available {
            "This terminal profile is unavailable."
        } else if self.id == shellfox_terminal::elevated::ADMINISTRATOR_PROFILE_ID {
            "Prompts for Windows administrator approval for this tab only."
        } else {
            &self.label
        })
    }
}

pub(crate) fn is_tab_menu_profile(id: &str) -> bool {
    !cfg!(windows) || id != "cmd"
}

impl Root {
    pub(crate) fn open_profile_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let epoch = self.navigation.epoch();
        let choices: Vec<_> = self
            .profiles
            .iter()
            .filter(|profile| is_tab_menu_profile(&profile.id))
            .map(|profile| Choice {
                label: profile.label.clone(),
                id: profile.id.clone(),
                available: profile.available,
                reason: profile.unavailable_reason.clone(),
                focus: cx.focus_handle(),
            })
            .collect();
        let previous_focus = match self.profile_menu.take() {
            Some(previous) => previous.previous_focus,
            None => window.focused(cx),
        };
        let focus = cx.focus_handle();
        choices
            .iter()
            .find(|choice| choice.available)
            .map_or(&focus, |choice| &choice.focus)
            .focus(window);
        self.profile_menu = Some(ProfileMenu {
            position,
            session,
            epoch,
            focus,
            choices,
            previous_focus,
            keyboard_selection: None,
        });
        self.session_menu = None;
        self.tab_menu = None;
        cx.notify();
    }

    fn dismiss_profile_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.profile_menu.take() {
            if menu.focus.contains_focused(window, cx)
                && let Some(focus) = menu.previous_focus
            {
                focus.focus(window);
            }
            cx.notify();
        }
    }

    fn choose_terminal_profile(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.profile_menu.as_ref() else {
            return;
        };
        let Some(choice) = menu.choices.get(index) else {
            return;
        };
        if !choice.available
            || !self
                .profiles
                .iter()
                .any(|profile| profile.id == choice.id && profile.available)
            || self.user_action_pending
            || self.navigation.epoch() != menu.epoch
            || self.closing_window
        {
            return;
        }
        let (id, session) = (choice.id.clone(), menu.session);
        self.dismiss_profile_menu(window, cx);
        if let Ok(profile_id) = ProfileId::new(id) {
            self.submit_native(
                NativeAction::Metadata(HostRequest::AddTab {
                    session_id: session,
                    input: AddTab {
                        title: None,
                        cwd: None,
                        profile_id,
                    },
                }),
                AfterAction::NewTab,
                window,
                cx,
            );
        }
    }

    fn profile_menu_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if matches!(key, "escape" | "tab") {
            self.dismiss_profile_menu(window, cx);
            if key == "tab" {
                if event.keystroke.modifiers.shift {
                    window.focus_prev();
                } else {
                    window.focus_next();
                }
            }
        } else if matches!(key, "up" | "down" | "home" | "end" | "enter" | "space") {
            let Some(menu) = &mut self.profile_menu else {
                return;
            };
            let enabled: Vec<_> = menu
                .choices
                .iter()
                .enumerate()
                .filter_map(|(index, choice)| {
                    (choice.available && !self.user_action_pending).then_some(index)
                })
                .collect();
            if let Some(&first) = enabled.first() {
                let current = menu
                    .keyboard_selection
                    .and_then(|selected| enabled.iter().position(|index| *index == selected))
                    .unwrap_or(0);
                let next = match key {
                    "up" => enabled[(current + enabled.len() - 1) % enabled.len()],
                    "down" => enabled[(current + 1) % enabled.len()],
                    "home" => first,
                    "end" => enabled[enabled.len() - 1],
                    _ => {
                        self.choose_terminal_profile(enabled[current], window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                };
                menu.keyboard_selection = Some(next);
                menu.choices[next].focus.focus(window);
                cx.notify();
            }
        } else {
            return;
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    pub(crate) fn render_profile_menu(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(menu) = &self.profile_menu else {
            return div().into_any_element();
        };
        let viewport = window.viewport_size();
        let style = TextStyle {
            font_family: crate::ui_font::family().into(),
            ..Default::default()
        };
        let width = menu
            .choices
            .iter()
            .map(|choice| {
                let run = style.to_run(choice.label.len());
                f32::from(
                    window
                        .text_system()
                        .shape_line(choice.label.clone().into(), px(12.), &[run], None)
                        .width,
                )
            })
            .fold(0., f32::max);
        let width = (width.ceil() + 26.).clamp(144., 280.);
        let height = (menu.choices.len() as f32 * 28. + 10.).min(f32::from(viewport.height) - 8.);
        let x =
            f32::from(menu.position.x).clamp(4., (f32::from(viewport.width) - width - 4.).max(4.));
        let y = f32::from(menu.position.y)
            .clamp(4., (f32::from(viewport.height) - height - 4.).max(4.));
        let mut items = div()
            .id("terminal-profile-menu")
            .track_focus(&menu.focus)
            .absolute()
            .occlude()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .max_h(px(height))
            .overflow_y_scroll()
            .p(px(4.))
            .rounded(px(6.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.4),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
            }])
            .on_mouse_down_out(
                cx.listener(|root, _, window, cx| root.dismiss_profile_menu(window, cx)),
            )
            .on_key_down(cx.listener(|root, event: &gpui::KeyDownEvent, window, cx| {
                root.profile_menu_key(event, window, cx)
            }));

        for (index, choice) in menu.choices.iter().enumerate() {
            let enabled = choice.available && !self.user_action_pending;
            let row = div()
                .id(("profile-choice", index))
                .relative()
                .w_full()
                .h(px(28.))
                .flex()
                .items_center()
                .px(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .rounded(px(4.))
                .text_color(palette.foreground)
                .child(div().truncate().child(choice.label.clone()))
                .tooltip(tooltip(choice.hint().to_owned(), palette))
                .when(!enabled, |style| style.opacity(0.4))
                .when(enabled, |item| {
                    item.cursor_pointer()
                        .track_focus(&choice.focus)
                        .hover(|style| style.bg(palette.hover))
                        .on_click(cx.listener(move |root, _, window, cx| {
                            root.choose_terminal_profile(index, window, cx)
                        }))
                })
                .when(enabled && menu.keyboard_selection == Some(index), |item| {
                    item.child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(palette.accent),
                    )
                });
            items = items.child(self.profile_choice_element(menu, index, row, enabled));
        }
        self.profile_menu_element(menu, items)
    }
}
