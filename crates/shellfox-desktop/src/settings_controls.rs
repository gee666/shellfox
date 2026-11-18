use super::*;
#[derive(Clone)]
pub(super) enum Action {
    CloseSettings,
    SelectCategory(Category),
    ToggleUpdates,
    Accent(String),
    Background(String),
    ChooseColor(api::ColorKind),
    SaveColor(api::ColorKind),
    Profiles,
    SelectProfile(String),
    Environment,
    SaveEnvironment,
    CancelEnvironment,
    AddAgent,
    ToggleAgent(String),
    RemoveAgent(String),
    Remote(remote::Operation),
    Ssh(ssh::Operation),
}
pub(super) fn column(weight: f32) -> gpui::Div {
    let mut element = div().flex_basis(px(0.)).min_w_0();
    element.style().flex_grow = Some(weight);
    element
}
pub(super) fn error(message: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .mt(px(4.))
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(gpui::rgb(0xfc8397))
        .child(message.into())
}
pub(super) fn heading(text: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .line_height(px(16.))
        .mb(px(10.))
        .child(text.into())
}
pub(super) fn hint(text: impl Into<gpui::SharedString>, p: Palette) -> gpui::Div {
    div()
        .text_size(px(11.))
        .line_height(px(17.))
        .text_color(p.muted)
        .mb(px(8.))
        .child(text.into())
}
pub(super) fn section(p: Palette) -> gpui::Div {
    div()
        .mt(px(22.))
        .pt(px(20.))
        .border_t_1()
        .border_color(p.border)
}
impl SettingsPanel {
    pub(super) fn bare_button(
        &mut self,
        id: impl Into<String>,
        label: impl Into<gpui::SharedString>,
        action: Action,
        cx: &mut Context<Self>,
    ) -> semantic_control::Control {
        let allowed = self.control_available(&action, cx);
        let id = id.into();
        let label: gpui::SharedString = label.into();
        let p = self.palette;
        let focus = self
            .controls
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let enabled = std::rc::Rc::new(std::cell::Cell::new(allowed));
        let semantic = self.control_semantic(&id, &label, &action, enabled.clone(), focus.clone());
        let click_enabled = enabled.clone();
        let key_enabled = enabled.clone();
        let key_action = action.clone();
        let click_focus = focus.clone();
        let element = div()
            .id(gpui::SharedString::from(id))
            .track_focus(&focus)
            .cursor_pointer()
            .when(!allowed, |style| {
                style.opacity(0.4).cursor(gpui::CursorStyle::Arrow)
            })
            .min_h(px(26.))
            .px(px(8.))
            .py(px(4.))
            .border_1()
            .border_color(p.border)
            .rounded(px(3.))
            .bg(p.surface)
            .text_size(px(12.))
            .text_color(p.foreground)
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |panel, _, window, cx| {
                if !click_enabled.get() {
                    return;
                }
                click_focus.focus(window);
                panel.act(action.clone(), window, cx)
            }))
            .on_key_down(
                cx.listener(move |panel, event: &gpui::KeyDownEvent, window, cx| {
                    if key_enabled.get()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        panel.act(key_action.clone(), window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }),
            )
            .child(label);
        semantic_control::Control::new(element, semantic, enabled)
    }
    pub(super) fn button(
        &mut self,
        id: impl Into<String>,
        label: impl Into<gpui::SharedString>,
        action: Action,
        cx: &mut Context<Self>,
    ) -> semantic_control::Control {
        let p = self.palette;
        self.bare_button(id, label, action, cx)
            .hover(move |style| style.bg(p.hover))
    }
    pub(super) fn text_button(
        &mut self,
        id: impl Into<String>,
        label: impl Into<gpui::SharedString>,
        action: Action,
        cx: &mut Context<Self>,
    ) -> semantic_control::Control {
        let p = self.palette;
        self.bare_button(id, label, action, cx)
            .border_0()
            .bg(gpui::transparent_black())
            .px(px(4.))
            .text_color(p.muted)
            .hover(move |style| style.bg(gpui::transparent_black()).text_color(p.accent))
    }
    pub(super) fn toggle(
        &mut self,
        id: String,
        checked: bool,
        action: Action,
        cx: &mut Context<Self>,
    ) -> semantic_control::Control {
        let p = self.palette;
        self.bare_button(id, "", action, cx)
            .role(crate::accessibility::Role::Switch)
            .pressed(checked)
            .when(!checked, |style| {
                style.hover(move |style| style.bg(p.hover))
            })
            .w(px(28.))
            .h(px(16.))
            .min_h(px(16.))
            .flex_shrink_0()
            .p(px(2.))
            .rounded(px(8.))
            .border_color(if checked { p.accent } else { p.input_border })
            .bg(if checked { p.accent } else { p.surface })
            .justify_start()
            .child(
                div()
                    .w(px(10.))
                    .h(px(10.))
                    .flex_shrink_0()
                    .rounded_full()
                    .when(checked, |style| style.ml(px(12.)))
                    .bg(if checked { gpui::white() } else { p.muted }),
            )
    }
    pub(super) fn act(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        match action {
            Action::CloseSettings => self.request_close(cx),
            Action::SelectCategory(category) => {
                self.category = category;
                let index = Category::ALL
                    .iter()
                    .position(|item| *item == category)
                    .unwrap_or(0);
                self.tab_focus[index].focus(window);
                cx.notify();
            }
            Action::ToggleUpdates => {
                if let Some(draft) = &self.draft {
                    self.stage(
                        SettingsPatch {
                            auto_update: Some(!draft.value.auto_update),
                            ..Default::default()
                        },
                        cx,
                    );
                }
            }
            Action::Accent(color) => self.stage(
                SettingsPatch {
                    accent_color: Some(color),
                    ..Default::default()
                },
                cx,
            ),
            Action::Background(color) => self.stage(
                SettingsPatch {
                    background_color: Some(color),
                    ..Default::default()
                },
                cx,
            ),
            Action::Profiles => {
                let Some(catalog) = &self.catalog else {
                    self.auxiliary_error =
                        Some("Terminal profile information is unavailable.".into());
                    cx.notify();
                    return;
                };
                let available = catalog
                    .profiles
                    .iter()
                    .filter(|profile| profile.available && !profile.elevation)
                    .collect::<Vec<_>>();
                if available.is_empty() {
                    return;
                }
                let selected = self
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.value.terminal_profile_id.as_ref())
                    .and_then(Option::as_ref)
                    .or(catalog.default_profile_id.as_ref());
                self.profile_index = available
                    .iter()
                    .position(|profile| Some(&profile.id) == selected)
                    .unwrap_or(0);
                self.profile_menu = !self.profile_menu;
                cx.notify();
            }
            Action::SelectProfile(id) => {
                if self.catalog.as_ref().is_some_and(|catalog| {
                    catalog
                        .profiles
                        .iter()
                        .any(|p| p.id == id && p.available && !p.elevation)
                }) {
                    self.profile_menu = false;
                    self.stage(
                        SettingsPatch {
                            terminal_profile_id: Some(Some(id)),
                            ..Default::default()
                        },
                        cx,
                    );
                }
            }
            Action::ChooseColor(kind) => self.choose_color(kind, cx),
            Action::SaveColor(kind) => self.save_color(kind, cx),
            Action::Environment => self.open_environment(window, cx),
            Action::SaveEnvironment => self.save_environment(cx),
            Action::CancelEnvironment => self.close_environment(window, cx),
            Action::AddAgent => self.add_agent(cx),
            Action::ToggleAgent(id) => {
                if let Some(draft) = &self.draft {
                    let mut rules = draft.value.process_rules.clone();
                    for rule in &mut rules {
                        if rule.id == id {
                            rule.enabled = !rule.enabled;
                        }
                    }
                    self.stage(
                        SettingsPatch {
                            process_rules: Some(rules),
                            ..Default::default()
                        },
                        cx,
                    );
                }
            }
            Action::RemoveAgent(id) => {
                if let Some(draft) = &self.draft {
                    let rules = draft
                        .value
                        .process_rules
                        .iter()
                        .filter(|rule| rule.id != id || rules::builtin_label(rule).is_some())
                        .cloned()
                        .collect();
                    self.stage(
                        SettingsPatch {
                            process_rules: Some(rules),
                            ..Default::default()
                        },
                        cx,
                    );
                }
            }
            Action::Remote(operation) => self.remote_action(operation, cx),
            Action::Ssh(operation) => self.ssh_action(operation, window, cx),
        }
    }
}
