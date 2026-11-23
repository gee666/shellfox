//! Opening the color popup from the Accent and Background rows.
use super::*;

impl SettingsPanel {
    pub(in crate::settings) fn choose_color(
        &mut self,
        kind: api::ColorKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operations.frozen() || self.color_picker.is_some() {
            return;
        }
        let Some(draft) = &self.draft else { return };
        let (value, presets, saved): (_, &[&str], _) = match kind {
            api::ColorKind::Accent => (
                &draft.value.accent_color,
                &colors::ACCENTS,
                self.custom_colors.as_ref().map(|colors| &colors.accent),
            ),
            api::ColorKind::Background => (
                &draft.value.background_color,
                &colors::BACKGROUNDS,
                self.custom_colors.as_ref().map(|colors| &colors.background),
            ),
        };
        let Some(initial) = parse_hex(value) else {
            return;
        };
        let presets = presets
            .iter()
            .filter_map(|color| parse_hex(color))
            .collect();
        let saved = saved
            .into_iter()
            .flatten()
            .filter_map(|color| parse_hex(color))
            .collect();
        let (anchor, palette) = (self.color_anchor(kind).clone(), self.palette);
        let view = cx.new(|cx| Picker::new(initial, presets, saved, palette, anchor, window, cx));
        let opener = format!("{kind:?}-chooser");
        let subscription = cx.subscribe_in(&view, window, move |panel, _, event, window, cx| {
            match event {
                Event::Close => {
                    panel.color_picker = None;
                    if let Some(focus) = panel.controls.get(&opener) {
                        focus.focus(window);
                    }
                }
                Event::Color(rgb) => {
                    let hex = hex(*rgb);
                    let patch = match kind {
                        api::ColorKind::Accent => SettingsPatch {
                            accent_color: Some(hex),
                            ..Default::default()
                        },
                        api::ColorKind::Background => SettingsPatch {
                            background_color: Some(hex),
                            ..Default::default()
                        },
                    };
                    panel.stage(patch, cx);
                }
            }
            cx.notify();
        });
        self.color_picker = Some(Active {
            view,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(in crate::settings) fn color_anchor(&self, kind: api::ColorKind) -> &Anchor {
        match kind {
            api::ColorKind::Accent => &self.accent_anchor,
            api::ColorKind::Background => &self.background_anchor,
        }
    }
}

pub(in crate::settings) fn remember_anchor(anchor: Anchor) -> impl IntoElement {
    gpui::canvas(
        move |bounds, _, _| anchor.set(Some(bounds)),
        |_, _, _, _| {},
    )
    .absolute()
    .left(px(-1.))
    .right(px(-1.))
    .top(px(-1.))
    .bottom(px(-1.))
}
