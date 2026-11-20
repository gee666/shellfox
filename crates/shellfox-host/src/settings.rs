use crate::{settings_store, storage, *};
use std::sync::Arc;

/// Trusted host adapter, not a remote request or a command launcher. Implementors
/// resolve a discovered, available, non-elevated profile, reject arbitrary shell
/// executables before returning canonical settings.
/// This callback must not mutate host state or call back into Host.
pub trait SettingsRuntime: Send + Sync {
    fn normalize(&self, previous: &Settings, candidate: Settings) -> Result<Settings>;
    /// Final owned-write decision after SQL writes, immediately before commit.
    /// Called under the Host lock/transaction. Never call back into Host here.
    fn check_commit(&self) -> Result<()> {
        Ok(())
    }
}
impl Host {
    pub fn install_settings_runtime(&self, runtime: Arc<dyn SettingsRuntime>) -> Result<()> {
        self.lock()?.settings_runtime = Some(runtime);
        Ok(())
    }
    /// Unlike Snapshot::preferences, this is the complete version-one global DTO.
    pub fn global_settings(&self) -> Result<Settings> {
        let inner = self.lock()?;
        settings_store::read(&inner.db)
    }
    /// Patch omissions preserve the latest committed values, including terminalEnv.
    /// Device fonts/layout/directories are deliberately not accepted here.
    pub fn save_global_settings(&self, patch: SettingsPatch) -> Result<Settings> {
        let mut inner = self.lock()?;
        let runtime = inner.settings_runtime.clone();
        let tx = inner
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage::storage_error)?;
        let previous = settings_store::read(&tx)?;
        let candidate = previous.patched(patch)?;
        let launch_changed = candidate.terminal_profile_id != previous.terminal_profile_id
            || candidate.shell_id != previous.shell_id
            || candidate.shell_executable != previous.shell_executable
            || candidate.adapter_id != previous.adapter_id;
        let settings = if let Some(runtime) = runtime.as_ref() {
            runtime.normalize(&previous, candidate)?
        } else {
            if launch_changed {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Terminal settings require the host discovery adapter.",
                ));
            }
            candidate
        };
        settings.validate()?;
        validate_env(&settings.terminal_env, cfg!(windows))?;
        if settings != previous {
            let mut data = storage::load(&tx)?;
            settings_store::project(&settings, &mut data)?;
            data.state.revision = data
                .state
                .revision
                .checked_add(1)
                .ok_or_else(|| storage::storage_error("revision overflow"))?;
            data.state.last_change = Some(ChangedEvent {
                revision: data.state.revision,
                reason: ChangeReason::Settings,
                select_session_id: data.state.selection.session_id,
            });
            settings_store::write(&tx, &settings)?;
            storage::save(&tx, &data)?;
        }
        if let Some(runtime) = runtime.as_ref() {
            runtime.check_commit()?;
        }
        tx.commit().map_err(storage::storage_error)?;
        Ok(settings)
    }
}

/// Full settings have a separate response contract; metadata requests continue
/// returning Snapshot until their existing callers migrate.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "method", content = "payload", deny_unknown_fields)]
pub enum SettingsRequest {
    #[serde(rename = "getGlobalSettings")]
    Get {},
    #[serde(rename = "saveGlobalSettings")]
    Save(Box<SettingsPatch>),
}
impl Host {
    pub fn settings_request(&self, request: SettingsRequest) -> Result<Settings> {
        match request {
            SettingsRequest::Get {} => self.global_settings(),
            SettingsRequest::Save(patch) => self.save_global_settings(*patch),
        }
    }
    pub fn settings_request_json(&self, input: &str) -> Reply<Settings> {
        let result = (|| {
            validation::ensure(
                input.len() <= 1024 * 1024,
                "Settings request exceeds the byte limit.",
            )?;
            let request = serde_json::from_str(input).map_err(|_| {
                Error::new(ErrorCode::Validation, "Invalid global settings request.")
            })?;
            self.settings_request(request)
        })();
        result.into()
    }
}
