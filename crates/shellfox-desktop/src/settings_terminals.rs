use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn terminals(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(settings) = self.draft.as_ref().map(|draft| draft.value.clone()) else {
            return div().into_any_element();
        };
        let p = self.palette;
        let selected = settings
            .terminal_profile_id
            .as_ref()
            .and_then(Option::as_deref)
            .or_else(|| {
                self.catalog
                    .as_ref()
                    .and_then(|catalog| catalog.default_profile_id.as_deref())
            });
        let selected_profile = self
            .catalog
            .as_ref()
            .and_then(|catalog| {
                catalog
                    .profiles
                    .iter()
                    .find(|profile| Some(profile.id.as_str()) == selected)
            })
            .cloned();
        let missing_selected =
            selected.is_some() && self.catalog.is_some() && selected_profile.is_none();
        let label = selected_profile
            .as_ref()
            .map(|profile| profile.label.clone())
            .or_else(|| selected.map(str::to_owned))
            .unwrap_or_else(|| {
                if self.catalog.is_some() {
                    "No shells available"
                } else if self.catalog_error.is_some() {
                    "Terminal profiles unavailable"
                } else if self.services.is_some() {
                    "Reading terminal profiles…"
                } else {
                    "Terminal profiles unavailable"
                }
                .into()
            });
        let selector = self
            .bare_button("default-shell", label.clone(), Action::Profiles, cx)
            .role(crate::accessibility::Role::ComboBox)
            .value(label)
            .expanded(self.profile_menu)
            .w_full()
            .max_w(px(380.))
            .h(px(28.))
            .min_h(px(28.))
            .px(px(6.))
            .py(px(2.))
            .bg(p.input_bg)
            .border_color(p.input_border)
            .justify_between()
            .child(crate::assets::icon("down").w(px(12.)).h(px(12.)));
        let mut default = div()
            .capture_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    panel.profile_key(event, window, cx)
                }),
            )
            .relative()
            .child(controls::heading("Default shell"))
            .child(selector)
            .child(controls::hint("Used for new local terminals.", p).mt(px(7.)));
        if let Some(error) = self.field_error(errors::Field::Shell) {
            default = default.child(controls::error(error));
        }
        if let Some(profile) = &selected_profile
            && !profile.available
        {
            default = default.child(
                controls::hint(
                    profile
                        .unavailable_reason
                        .clone()
                        .unwrap_or_else(|| "This profile is unavailable.".into()),
                    p,
                )
                .text_color(gpui::rgb(0xfc8397)),
            );
        }
        if missing_selected {
            default = default.child(
                controls::hint("The saved profile is unavailable on this host.", p)
                    .text_color(gpui::rgb(0xfc8397)),
            );
        }
        if self.profile_menu {
            let active = self
                .catalog
                .as_ref()
                .and_then(|catalog| {
                    catalog
                        .profiles
                        .iter()
                        .filter(|profile| profile.available && !profile.elevation)
                        .nth(self.profile_index)
                })
                .map(|profile| profile.id.clone());
            let mut menu = div()
                .id("settings-profile-options")
                .max_h(px(240.))
                .overflow_y_scroll()
                .absolute()
                .top(px(54.))
                .left_0()
                .w_full()
                .max_w(px(380.))
                .p(px(4.))
                .bg(p.panel)
                .border_1()
                .border_color(p.border)
                .rounded(px(6.))
                .shadow_lg();
            let profiles = self
                .catalog
                .as_ref()
                .map(|catalog| catalog.profiles.clone())
                .unwrap_or_default();
            for profile in profiles.into_iter().filter(|profile| !profile.elevation) {
                let available = profile.available;
                let focused = active.as_ref() == Some(&profile.id);
                menu = menu.child(
                    self.text_button(
                        format!("profile-{}", profile.id),
                        profile.label,
                        Action::SelectProfile(profile.id),
                        cx,
                    )
                    .w_full()
                    .justify_start()
                    .px(px(8.))
                    .py(px(6.))
                    .when(focused, |style| style.bg(p.hover))
                    .when(!available, |style| style.opacity(0.4)),
                );
            }
            default = default.child(menu);
        }
        let environment = self.text_button(
            "all-terminal-environment",
            "Add Environment Variables (all terminals)",
            Action::Environment,
            cx,
        );
        let mut page = div()
            .child(default)
            .child(controls::section(p).child(environment));
        if self
            .catalog
            .as_ref()
            .is_some_and(|catalog| catalog.platform == "linux")
        {
            page = page.child(
                controls::section(p)
                    .child(controls::heading("Python"))
                    .child(div().max_w(px(380.)).child(self.bind_field(
                        self.forms.fields[&Input::Python].clone(),
                        "Python path",
                        None,
                        cx,
                    )))
                    .child(
                        controls::hint(
                            "Used to safely close terminals. Leave empty for auto-detect.",
                            p,
                        )
                        .mt(px(7.)),
                    ),
            );
            if let Some(error) = self.field_error(errors::Field::Python) {
                page = page.child(controls::error(error));
            }
            if let Some(error) = self
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.python_reason.as_ref())
            {
                page = page.child(controls::hint(error.clone(), p));
            }
        }
        if let Some(error) = &self.catalog_error {
            page = page.child(controls::hint(error.clone(), p).text_color(gpui::rgb(0xfc8397)));
        }
        page.into_any_element()
    }
}
