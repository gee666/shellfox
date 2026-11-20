use super::*;

/// The complete persisted settings DTO has the same validated v1 wire fields.
/// This preserves omitted vs explicit-null profiles and all host-only settings.
pub fn persisted_settings(settings: &core::Settings) -> AdapterResult<wire::SettingsDto> {
    settings
        .validate()
        .map_err(|_| AdapterError::InvalidFacts {
            context: "persisted global settings",
        })?;
    let bytes = serde_json::to_vec(settings).map_err(|_| AdapterError::InvalidFacts {
        context: "persisted global settings",
    })?;
    wire::decode(&bytes).map_err(|_| AdapterError::InvalidFacts {
        context: "global settings wire DTO",
    })
}
pub fn manager_snapshot_with_settings(
    snapshot: &core::Snapshot,
    facts: &ManagerFacts,
    settings: &core::Settings,
) -> AdapterResult<wire::ManagerSnapshot> {
    manager_snapshot_with_settings_visible(snapshot, facts, settings, &Default::default())
}
pub fn manager_snapshot_with_settings_visible(
    snapshot: &core::Snapshot, facts: &ManagerFacts, settings: &core::Settings, private: &std::collections::HashSet<core::TabId>,
) -> AdapterResult<wire::ManagerSnapshot> {
    let settings = persisted_settings(settings)?;
    let mut actual = facts.clone();
    actual.settings = Some(SettingsFacts {
        adapter_id: settings.adapter_id,
        shell_id: settings.shell_id,
        shell_executable: settings.shell_executable.clone(),
        process_rules: settings.process_rules.clone(),
        history_page_size: settings.history_page_size,
        session_settle_hours: settings.session_settle_hours,
        session_delete_hours: settings.session_delete_hours,
        auto_update: settings.auto_update,
        terminal_env: settings.terminal_env.clone(),
    });
    let mut dto = manager_snapshot_visible(snapshot, &actual, private)?;
    dto.settings = settings;
    checked(dto, "manager with persisted global settings")
}
