//! Called only inside legacy's validated read-only transaction. No PTY, PID,
//! registration, command, binding or session runtime state crosses this boundary.
use crate::*;
use rusqlite::Connection;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsImportPolicy {
    /// Safe default. Environment values may contain secrets and are not copied.
    #[default]
    ExcludeEnvironment,
    /// Caller explicitly approved copying global environment, including secrets.
    IncludeGlobalEnvironment,
}
impl Host {
    /// Import settings only from a private snapshot, without importing sessions or
    /// replacing settings in a Rust store that has already been used.
    pub fn import_electron_settings(&self, snapshot: &std::path::Path) -> Result<bool> {
        use rusqlite::OpenFlags;
        let db = Connection::open_with_flags(
            snapshot,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(crate::storage::storage_error)?;
        db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
            .map_err(crate::storage::storage_error)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(crate::storage::storage_error)?;
        validation::ensure(
            (1..=5).contains(&version),
            "Unsupported Electron database version.",
        )?;
        let kind: String = db
            .query_row(
                "SELECT type FROM pragma_table_list WHERE schema='main' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .map_err(crate::storage::storage_error)?;
        validation::ensure(kind == "table", "Invalid Electron settings table.")?;
        let settings = read(&db, SettingsImportPolicy::IncludeGlobalEnvironment)?;
        let mut inner = self.lock()?;
        let tx = inner
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(crate::storage::storage_error)?;
        let mut data = crate::storage::load(&tx)?;
        if data.state.revision != 0 {
            return Ok(false);
        }
        crate::settings_store::project(&settings, &mut data)?;
        data.state.revision = 1;
        data.state.last_change = Some(ChangedEvent {
            revision: 1,
            reason: ChangeReason::Settings,
            select_session_id: data.state.selection.session_id,
        });
        crate::settings_store::write(&tx, &settings)?;
        crate::storage::save(&tx, &data)?;
        tx.commit().map_err(crate::storage::storage_error)?;
        Ok(true)
    }

    /// Import a COPY of an Electron database. Profile/Python configuration is
    /// retained even when unavailable here; discovery still gates later launch.
    /// Passwords/private credentials are not settings and are never imported.
    /// Only explicit IncludeGlobalEnvironment consent copies possible env secrets.
    pub fn import_legacy_with_settings(
        &self,
        path: impl AsRef<std::path::Path>,
        policy: SettingsImportPolicy,
    ) -> Result<Snapshot> {
        let (path, _checked_artifacts) = crate::storage_guard::legacy(path.as_ref())?;
        let imported = crate::legacy::read_legacy(&path, policy)?;
        let mut inner = self.lock()?;
        let runtime = inner.settings_runtime.clone();
        let tx = inner
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(crate::storage::storage_error)?;
        let mut data = crate::storage::load(&tx)?;
        if !data.sessions.is_empty() || data.state.revision != 0 {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Legacy import requires a new empty experiment store.",
            ));
        }
        data.sessions = imported.sessions;
        data.creation_facts = imported.creation_facts;
        crate::settings_store::project(&imported.settings, &mut data)?;
        data.repair_selection();
        data.state.revision = 1;
        data.state.last_change = Some(ChangedEvent {
            revision: 1,
            reason: ChangeReason::Settings,
            select_session_id: data.state.selection.session_id,
        });
        crate::settings_store::write(&tx, &imported.settings)?;
        crate::storage::save(&tx, &data)?;
        let snapshot = data.snapshot();
        if let Some(runtime) = runtime {
            runtime.check_commit()?;
        }
        tx.commit().map_err(crate::storage::storage_error)?;
        Ok(snapshot)
    }
}
pub(crate) fn read(db: &Connection, policy: SettingsImportPolicy) -> Result<Settings> {
    let count: i64 = db
        .query_row("SELECT count(*) FROM settings", [], |r| r.get(0))
        .map_err(crate::storage::storage_error)?;
    validation::ensure(count == 1, "Legacy settings must contain one row.")?;
    let encoded: String = db
        .query_row(
            "SELECT value FROM settings WHERE id=1 AND length(CAST(value AS BLOB))<=4194304",
            [],
            |r| r.get(0),
        )
        .map_err(crate::storage::storage_error)?;
    let mut value: serde_json::Value = serde_json::from_str(&encoded)
        .map_err(|_| Error::new(ErrorCode::Validation, "Invalid legacy settings."))?;
    if policy == SettingsImportPolicy::ExcludeEnvironment {
        // Do not deserialize old multiline/invalid/secret environment values.
        let object = value
            .as_object_mut()
            .ok_or_else(|| Error::new(ErrorCode::Validation, "Invalid legacy settings."))?;
        object.insert("terminalEnv".into(), serde_json::json!([]));
    }
    let mut settings: Settings = serde_json::from_value(value)
        .map_err(|_| Error::new(ErrorCode::Validation, "Invalid legacy settings."))?;
    settings.validate()?;
    validate_env(&settings.terminal_env, cfg!(windows))?;
    settings.upgrade_bundled_rules();
    settings.validate()?;
    Ok(settings)
}
