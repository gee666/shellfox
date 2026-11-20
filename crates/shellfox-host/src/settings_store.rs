//! Global settings are authoritative. host_state's appearance/profile/env fields
//! remain a compatibility projection for existing snapshot and launch callers.
use crate::{storage, *};
use rusqlite::Connection;

pub(crate) fn migrate(db: &Connection, existing: bool) -> Result<()> {
    if existing {
        validate_legacy_state(db)?;
    }
    db.execute_batch(
        "CREATE TABLE global_settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);",
    )
    .map_err(storage::storage_error)?;
    let mut data = storage::load(db)?;
    let mut settings = Settings::default();
    if existing {
        let prefs = &data.state.preferences;
        settings.accent_color = prefs.accent_color.clone();
        settings.background_color = prefs.background_color.clone();
        settings.terminal_profile_id = Some(Some(prefs.default_profile_id.to_string()));
        settings.shell_id = shell_for_profile(&prefs.default_profile_id);
        settings.terminal_env = data.state.terminal_env.clone();
    }
    write(db, &settings)?;
    project(&settings, &mut data)?;
    storage::save(db, &data)
}
pub(crate) fn read(db: &Connection) -> Result<Settings> {
    // Older Rust stores wrote the retired Python preference into global settings.
    let value: String = db
        .query_row(
            "SELECT json_remove(value,'$.pythonPath') FROM global_settings WHERE id=1 AND length(CAST(value AS BLOB))<=4194304",
            [],
            |r| r.get(0),
        )
        .map_err(storage::storage_error)?;
    let settings: Settings = serde_json::from_str(&value).map_err(storage::storage_error)?;
    settings.validate().map_err(storage::storage_error)?;
    validate_env(&settings.terminal_env, cfg!(windows)).map_err(storage::storage_error)?;
    Ok(settings)
}
pub(crate) fn write(db: &Connection, settings: &Settings) -> Result<()> {
    settings.validate()?;
    validate_env(&settings.terminal_env, cfg!(windows))?;
    let encoded = storage::encode(settings)?;
    validation::ensure(
        encoded.len() <= 4 * 1024 * 1024,
        "Global settings exceed the storage byte limit.",
    )?;
    db.execute("INSERT INTO global_settings VALUES(1,?) ON CONFLICT(id) DO UPDATE SET value=excluded.value", [encoded])
        .map_err(storage::storage_error)?;
    Ok(())
}
pub(crate) fn project(settings: &Settings, data: &mut storage::Data) -> Result<()> {
    data.state.preferences = settings.project_preferences(&data.state.preferences)?;
    data.state.terminal_env = settings.terminal_env.clone();
    Ok(())
}
/// Old APIs can still update appearance/profile/env. Merge only fields they
/// changed, never a stale complete projection, within the caller's transaction.
pub(crate) fn merge_compatibility(
    db: &Connection,
    old: &Preferences,
    old_env: &[EnvVar],
    after: &mut storage::Data,
) -> Result<()> {
    let mut settings = read(db)?;
    let new = &after.state.preferences;
    if old.accent_color != new.accent_color {
        settings.accent_color = new.accent_color.clone();
    }
    if old.background_color != new.background_color {
        settings.background_color = new.background_color.clone();
    }
    if old.default_profile_id != new.default_profile_id {
        settings.terminal_profile_id = Some(Some(new.default_profile_id.to_string()));
        settings.shell_id = shell_for_profile(&new.default_profile_id);
        // An old profile-intent request has no discovered executable authority.
        settings.shell_executable = None;
    }
    if old_env != after.state.terminal_env {
        settings.terminal_env = after.state.terminal_env.clone();
    }
    write(db, &settings)?;
    project(&settings, after)
}
fn validate_legacy_state(db: &Connection) -> Result<()> {
    // The local-only representation is legal only after schema2 migration.
    // Do not turn missing required fields in a corrupt schema1 into defaults.
    let raw: String = db
        .query_row("SELECT value FROM host_state WHERE id=1", [], |r| r.get(0))
        .map_err(storage::storage_error)?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(storage::storage_error)?;
    let prefs = value
        .get("preferences")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| storage::storage_error("invalid schema1"))?;
    if ![
        "fontFamily",
        "fontSize",
        "accentColor",
        "backgroundColor",
        "defaultProfileId",
    ]
    .iter()
    .all(|key| prefs.contains_key(*key))
        || value.get("terminalEnv").is_none()
    {
        return Err(storage::storage_error("invalid schema1"));
    }
    Ok(())
}
fn shell_for_profile(profile: &ProfileId) -> SettingsShell {
    match profile.as_str() {
        "pwsh" => SettingsShell::Pwsh,
        "windows-powershell" => SettingsShell::WindowsPowershell,
        id if id == "wsl" || id.starts_with("wsl:") => SettingsShell::Wsl,
        _ => SettingsShell::LoginShell,
    }
}
