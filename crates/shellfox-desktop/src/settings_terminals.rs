use super::{
    controls::{self, Action},
    *,
};
impl SettingsPanel {
    pub(super) fn terminals(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.palette;
        let selected_profile = self.selected_shell().cloned();
        let label = selected_profile
            .as_ref()
            .map(|profile| profile.label.clone())
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
            .pl(px(9.))
            .pr(px(2.))
            .py(px(2.))
            .bg(p.input_bg)
            .border_color(p.input_border)
            .justify_between()
            .capture_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    panel.profile_key(event, window, cx)
                }),
            )
            .child(crate::assets::icon_sized("down", 12.));
        let mut default = div()
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
        if self.catalog.is_some()
            && self.saved_shell_id().is_some_and(|id| {
                selected_profile
                    .as_ref()
                    .is_none_or(|profile| profile.id != id)
            })
        {
            default = default.child(controls::hint(
                "The saved profile is unavailable on this host.",
                p,
            ));
        }
        if self.profile_menu {
            let active = self.profile_index;
            let mut menu = div()
                .id("settings-profile-options")
                .max_h(px(240.))
                .overflow_y_scroll()
                .track_scroll(&self.profile_scroll)
                .absolute()
                .occlude()
                .on_mouse_down_out(cx.listener(|panel, _, window, cx| {
                    panel.close_profiles(window, cx);
                    cx.stop_propagation();
                }))
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
            let profiles = self.shell_profiles().cloned().collect::<Vec<_>>();
            for (index, profile) in profiles.into_iter().enumerate() {
                let selected = selected_profile
                    .as_ref()
                    .is_some_and(|p| p.id == profile.id);
                let option = self
                    .text_button(
                        format!("profile-{}", profile.id),
                        profile.label,
                        Action::SelectProfile(profile.id),
                        cx,
                    )
                    .skip_tab()
                    .role(crate::accessibility::Role::ListBoxOption)
                    .selected(selected)
                    .w_full()
                    .justify_start()
                    .px(px(8.))
                    .py(px(6.))
                    .when(index == active, |style| style.bg(p.hover))
                    .when_some(profile.unavailable_reason, |option, reason| {
                        option.tooltip(crate::assets::tooltip(reason, p))
                    });
                menu = menu.child(option);
            }
            default = default.child(gpui::deferred(menu).with_priority(40));
        }
        let environment = self.text_button(
            "all-terminal-environment",
            "Add Environment Variables (all terminals)",
            Action::Environment,
            cx,
        );
        let mut page = div()
            .child(default)
            .child(controls::section(p).child(div().flex().child(environment)));
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
