use super::*;
impl SettingsPanel {
    /// Supply one device-local store created from the validated STABLE desktop
    /// data directory. Neither host DBs nor per-run scratch are a device store.
    pub fn install_device_colors(
        &mut self,
        store: Arc<DeviceColors>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.device_colors.is_none(),
            "Device color storage is already installed."
        );
        self.device_colors = Some(store.clone());
        let task = cx.background_executor().spawn(async move { store.read() });
        cx.spawn(async move |panel, cx| {
            let result = task.await;
            let _ = panel.update(cx, |panel, cx| {
                let result = result.inspect_err(|_| {
                    panel.custom_colors = Some(api::CustomColors::default());
                });
                match result {
                    Ok(colors) => {
                        panel.custom_colors = Some(colors);
                        panel.auxiliary_error = None;
                    }
                    Err(error) => panel.auxiliary_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        Ok(())
    }
    pub(super) fn save_color(&mut self, kind: api::ColorKind, cx: &mut Context<Self>) {
        if self.operations.frozen() || self.device_color_saving {
            return;
        }
        let Some(draft) = &self.draft else {
            return;
        };
        let color = match kind {
            api::ColorKind::Accent => &draft.value.accent_color,
            api::ColorKind::Background => &draft.value.background_color,
        }
        .to_ascii_lowercase();
        if shellfox_core::validation::color(&color).is_err() {
            return;
        }
        let builtins: &[&str] = match kind {
            api::ColorKind::Accent => &colors::ACCENTS,
            api::ColorKind::Background => &colors::BACKGROUNDS,
        };
        if builtins.contains(&color.as_str()) {
            return;
        }
        let Some(store) = self.device_colors.clone() else {
            self.auxiliary_error = Some("Device-local color storage is not configured.".into());
            cx.notify();
            return;
        };
        let current = self
            .custom_colors
            .get_or_insert_with(api::CustomColors::default);
        colors::add(current, kind, &color);
        self.device_color_saving = true;
        let lease = self.operations.begin();
        let task = cx
            .background_executor()
            .spawn(async move { store.save(kind, &color) });
        cx.spawn(async move |panel, cx| {
            let result = task.await;
            let _ = panel.update(cx, |panel, cx| {
                drop(lease);
                panel.device_color_saving = false;
                match result {
                    Ok(colors) => {
                        panel.custom_colors = Some(colors);
                        panel.auxiliary_error = None;
                    }
                    Err(error) => panel.auxiliary_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
