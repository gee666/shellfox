use super::colors::{ACCENTS, BACKGROUNDS};
use super::{
    controls::{self, Action},
    *,
};
fn raw_color(value: &str) -> Option<gpui::Hsla> {
    if shellfox_core::validation::color(value).is_err() {
        return None;
    }
    u32::from_str_radix(&value[1..], 16)
        .ok()
        .map(|color| gpui::rgb(color).into())
}
impl SettingsPanel {
    pub(super) fn general(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(settings) = self.draft.as_ref().map(|draft| draft.value.clone()) else {
            return div().into_any_element();
        };
        let p = self.palette;
        // The source checkbox is a full-width inline input. Its native glyph is
        // centered above the wrapped label, not an invented left-aligned toggle.
        let check = self
            .bare_button("automatic-updates", "", Action::ToggleUpdates, cx)
            .role(crate::accessibility::Role::CheckBox)
            .pressed(settings.auto_update)
            .w_full()
            .h(px(35.))
            .min_h(px(35.))
            .p_0()
            .ml(px(4.))
            .border_0()
            .bg(gpui::transparent_black())
            .hover(|style| style.bg(gpui::transparent_black()))
            .child(
                div()
                    .w(px(28.))
                    .h(px(28.))
                    .rounded(px(2.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if settings.auto_update {
                        gpui::rgb(if p.dark { 0x9ac8ff } else { 0x0075ff }).into()
                    } else {
                        p.input_bg
                    })
                    .when(!settings.auto_update, |style| {
                        style.border_1().border_color(p.input_border)
                    })
                    .when(settings.auto_update, |style| {
                        style
                            .text_size(px(24.))
                            .text_color(gpui::rgb(if p.dark { 0x333333 } else { 0xffffff }))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("✓")
                    }),
            );
        let updates=div().child(controls::heading("Updates")).child(check)
            .child(div().id("automatic-updates-label").line_height(px(16.)).cursor_pointer().on_click(cx.listener(|panel,_,window,cx|panel.act(Action::ToggleUpdates,window,cx))).child("Automatic updates"))
            .child(controls::hint("Download new versions automatically and install when you quit. Turn off to update manually.",p).mb_0());
        let accent = self.swatches(api::ColorKind::Accent, &ACCENTS, &settings.accent_color, cx);
        let background = self.swatches(
            api::ColorKind::Background,
            &BACKGROUNDS,
            &settings.background_color,
            cx,
        );
        let mut history = controls::section(p).child(controls::heading("Session history"));
        for (index, key) in [Input::History, Input::Settle, Input::Delete]
            .into_iter()
            .enumerate()
        {
            let field = self.bind_field(self.forms.fields[&key].clone(), key.label(), None, cx);
            history = history.child(
                div()
                    .max_w(px(400.))
                    .min_h(px(32.))
                    .when(index > 0, |style| style.mt(px(4.)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(key.label())
                    .child(div().w(px(86.)).child(field)),
            );
            let field = match key {
                Input::History => errors::Field::History,
                Input::Settle => errors::Field::Settle,
                _ => errors::Field::Delete,
            };
            if let Some(error) = self.field_error(field) {
                history = history.child(controls::error(error));
            }
        }
        let mut page = div()
            .child(updates)
            .child(
                controls::section(p)
                    .child(controls::heading("Accent color"))
                    .child(accent)
                    .when_some(
                        self.field_error(errors::Field::Accent),
                        |section, message| section.child(controls::error(message)),
                    ),
            )
            .child(
                controls::section(p)
                    .child(controls::heading("Background color"))
                    .child(background)
                    .when_some(
                        self.field_error(errors::Field::Background),
                        |section, message| section.child(controls::error(message)),
                    )
                    .child(
                        controls::hint("Text and all other colors adapt automatically.", p)
                            .mt(px(10.))
                            .mb_0(),
                    ),
            )
            .child(history);
        if let Some(error) = &self.auxiliary_error {
            page = page.child(controls::error(error.clone()));
        }
        page.into_any_element()
    }
    fn swatches(
        &mut self,
        kind: api::ColorKind,
        presets: &[&str],
        value: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let mut row = div().flex().items_center().flex_wrap().gap(px(9.));
        for (index, color) in presets.iter().enumerate() {
            row =
                row.child(self.swatch(format!("{kind:?}-preset-{index}"), color, value, kind, cx));
        }
        let custom = self
            .custom_colors
            .as_ref()
            .map(|colors| match kind {
                api::ColorKind::Accent => colors.accent.clone(),
                api::ColorKind::Background => colors.background.clone(),
            })
            .unwrap_or_default();
        if !custom.is_empty() {
            let mut saved = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(8.))
                .border_l_1()
                .border_color(p.border);
            for (index, color) in custom.iter().enumerate() {
                saved = saved.child(self.swatch(
                    format!("{kind:?}-custom-{index}"),
                    color,
                    value,
                    kind,
                    cx,
                ));
            }
            row = row.child(saved);
        }
        let selected = raw_color(value).unwrap_or(p.background);
        row = row.child(
            self.bare_button(
                format!("{kind:?}-chooser"),
                "",
                Action::ChooseColor(kind),
                cx,
            )
            .role(crate::accessibility::Role::ColorWell)
            .value(value)
            .w(px(28.))
            .h(px(28.))
            .min_h(px(28.))
            .p(px(4.))
            .bg(p.input_bg)
            .border_color(p.input_border)
            .child(
                div()
                    .w_full()
                    .h(px(14.))
                    .bg(selected)
                    .border_1()
                    .border_color(p.muted),
            ),
        );
        let lower = value.to_ascii_lowercase();
        let can_add = !self.device_color_saving
            && self.custom_colors.is_some()
            && shellfox_core::validation::color(value).is_ok()
            && !presets.contains(&lower.as_str())
            && !custom.iter().any(|color| color.eq_ignore_ascii_case(value));
        row = row.child(
            self.text_button(
                format!("{kind:?}-save-custom"),
                "",
                Action::SaveColor(kind),
                cx,
            )
            .w(px(28.))
            .h(px(28.))
            .p_0()
            .disabled(!can_add)
            .when(!can_add, |style| style.opacity(0.4))
            .child(crate::assets::icon("plus")),
        );
        row.into_any_element()
    }
    fn swatch(
        &mut self,
        id: String,
        color: &str,
        value: &str,
        kind: api::ColorKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let action = match kind {
            api::ColorKind::Accent => Action::Accent(color.into()),
            api::ColorKind::Background => Action::Background(color.into()),
        };
        let fill = raw_color(color).unwrap_or(p.background);
        let selected = color.eq_ignore_ascii_case(value);
        self.bare_button(id, "", action, cx)
            .pressed(selected)
            .w(px(22.))
            .h(px(22.))
            .min_h(px(22.))
            .p_0()
            .rounded(px(4.))
            .bg(fill)
            .when(selected, |style| style.border_2())
            .when(!selected, |style| style.border_1())
            .border_color(if selected { p.foreground } else { p.border })
            .hover(move |style| style.bg(fill))
            .when(selected, |style| {
                style.child(
                    div()
                        .size_full()
                        .rounded(px(2.))
                        .border_2()
                        .border_color(p.panel),
                )
            })
            .into_any_element()
    }
}
