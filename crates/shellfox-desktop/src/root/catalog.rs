//! Project only the protected owner-bound launch catalog, never GUI filesystem observations.
use super::*;
impl Root {
    pub(crate) fn accept_catalog(
        &mut self,
        result: anyhow::Result<shellfox_service::NativeLaunchCatalog>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(catalog) => {
                self.workspace.profiles = catalog
                    .profiles
                    .profiles
                    .iter()
                    .map(|profile| crate::settings::api::Profile {
                        id: profile.id.clone(),
                        label: profile.label.clone(),
                        available: profile.available,
                        unavailable_reason: profile.unavailable_reason.clone().flatten(),
                        elevation: profile.elevation.is_some(),
                    })
                    .collect();
                if let Some(panel) = &self.settings.panel {
                    panel.update(cx, |panel, cx| panel.set_native_catalog(catalog, cx));
                }
            }
            Err(error) => {
                self.workspace.profiles.clear();
                let reason = format!("Owner launch catalog is unavailable: {error:#}");
                if let Some(panel) = &self.settings.panel {
                    panel.update(cx, |panel, cx| {
                        panel.set_catalog_unavailable(reason.clone(), cx)
                    });
                }
                self.error = Some(reason);
            }
        }
        cx.notify();
    }
    pub(crate) fn refresh_catalog(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.close.closing {
            return;
        }
        let Some(client) = self.host.clone() else {
            return;
        };
        self.workspace.catalog_epoch = self.workspace.catalog_epoch.wrapping_add(1);
        let epoch = self.workspace.catalog_epoch;
        cx.spawn_in(window, async move |root, cx| {
            let result = client.terminal_profiles().await;
            let _ = root.update_in(cx, |root, _, cx| {
                if !root.close.closing && root.workspace.catalog_epoch == epoch {
                    root.accept_catalog(result, cx);
                }
            });
        })
        .detach();
    }
}
