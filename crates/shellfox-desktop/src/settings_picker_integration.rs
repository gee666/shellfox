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
        let value = match kind {
            api::ColorKind::Accent => &draft.value.accent_color,
            api::ColorKind::Background => &draft.value.background_color,
        };
        let Some(initial) = value.strip_prefix('#').and_then(color::decode_hex) else {
            return;
        };
        let anchor = self.color_anchor(kind).clone();
        let view = cx.new(|cx| Picker::new(initial, anchor, window, cx));
        let opener = format!("{kind:?}-chooser");
        let subscription = cx.subscribe_in(&view, window, move |panel, _, event, window, cx| {
            match event {
                Event::Close => {
                    panel.color_picker = None;
                    if let Some(focus) = panel.controls.get(&opener) {
                        focus.focus(window);
                    }
                }
                Event::SamplingError(message) => panel.auxiliary_error = Some(message.clone()),
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
