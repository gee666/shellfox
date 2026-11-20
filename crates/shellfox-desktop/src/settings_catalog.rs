use super::*;
pub(super) fn validate(catalog: &api::Catalog) -> anyhow::Result<()> {
    fn profile_id(value: &str) -> bool {
        !value.is_empty()
            && value.encode_utf16().count() <= 200
            && !value.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}')
    }
    anyhow::ensure!(
        matches!(catalog.platform.as_str(), "win32" | "linux" | "darwin"),
        "Invalid host platform response."
    );
    anyhow::ensure!(
        catalog.profiles.len() <= 100,
        "Invalid terminal profile catalog size."
    );
    let mut ids = std::collections::HashSet::new();
    for profile in &catalog.profiles {
        anyhow::ensure!(
            profile_id(&profile.id) && ids.insert(&profile.id),
            "Invalid terminal profile identity."
        );
        shellfox_core::validation::title(&profile.label)?;
        if let Some(reason) = &profile.unavailable_reason {
            shellfox_core::validation::text(reason, 1000)?;
        }
    }
    if let Some(id) = &catalog.default_profile_id {
        anyhow::ensure!(profile_id(id), "Invalid default terminal profile identity.");
    }
    Ok(())
}
impl SettingsPanel {
    /// Root passes the exact readonly reply from this window's originating client.
    /// This method does no discovery, filesystem access or source-fact guessing.
    pub fn set_native_catalog(
        &mut self,
        mut catalog: shellfox_service::NativeLaunchCatalog,
        cx: &mut Context<Self>,
    ) {
        if fixture::value("CATALOG").as_deref() == Some("empty") {
            catalog.profiles.profiles.clear();
            catalog.profiles.default_profile_id = None;
        }
        self.apply_catalog(
            api::Catalog {
                profiles: catalog
                    .profiles
                    .profiles
                    .into_iter()
                    .map(|profile| api::Profile {
                        id: profile.id,
                        label: profile.label,
                        available: profile.available,
                        unavailable_reason: profile.unavailable_reason.flatten(),
                        elevation: profile.elevation.is_some(),
                    })
                    .collect(),
                default_profile_id: catalog.profiles.default_profile_id,
                platform: catalog.probe.platform,
            },
            cx,
        );
    }
    fn apply_catalog(&mut self, catalog: api::Catalog, cx: &mut Context<Self>) {
        if let Err(error) = validate(&catalog) {
            self.set_catalog_unavailable(error.to_string(), cx);
            return;
        }
        self.catalog = Some(catalog);
        self.catalog_error = None;
        cx.notify();
    }

    pub fn set_catalog_unavailable(&mut self, message: String, cx: &mut Context<Self>) {
        self.catalog = None;
        self.catalog_error = Some(message);
        cx.notify();
    }
}
