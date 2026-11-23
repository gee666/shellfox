//! One live projector: immutable owner facts plus off-owner read-only discovery.
use super::{FactsDiscovery, NativeFactsDiscovery, SourceFacts};
use crate::{
    remote::{ManagerFactsFuture, ManagerFactsRequest, ManagerFactsResult, SourceFactsProjector},
    system::{HostSystemServices, SystemScope},
};
use shellfox_protocol as wire;
use shellfox_remote::adapter::{self, ManagerFacts, SettingsFacts};
use std::{path::PathBuf, sync::Arc};
#[cfg(test)]
mod tests;

pub struct NativeSourceFactsProjector {
    discovery: Arc<NativeFactsDiscovery>,
    system: Arc<HostSystemServices>,
    scope: SystemScope,
}
impl NativeSourceFactsProjector {
    /// supervisor is read-only resource evidence, never process execution authority.
    /// The same explicit system provider/scope is used by boot and owned fixtures.
    pub fn new(
        supervisor: PathBuf,
        system: Arc<HostSystemServices>,
        scope: SystemScope,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            discovery: Arc::new(NativeFactsDiscovery::new(supervisor)?),
            system,
            scope,
        })
    }
}
fn cancelled() -> wire::AppError {
    wire::AppError {
        code: wire::ErrorCode::NativeUnavailable,
        message: "Owned facts projection was cancelled.".into(),
        retryable: false,
    }
}
impl NativeSourceFactsProjector {
    fn project_mode(
        &self,
        source: Arc<SourceFacts>,
        request: ManagerFactsRequest,
        legacy_sessions: bool,
    ) -> ManagerFactsFuture<'_> {
        let discovery = Arc::clone(&self.discovery);
        let system = Arc::clone(&self.system);
        let scope = self.scope;
        Box::pin(async move {
            if !source.matches_request(&request) {
                return Err(wire::AppError {
                    code: wire::ErrorCode::TargetLost,
                    message: "Owned snapshot/settings/runtime facts do not match this request."
                        .into(),
                    retryable: false,
                });
            }
            let input = Arc::clone(&source);
            let worker = tokio::task::spawn_blocking(move || {
                if input.cancelled.is_cancelled() {
                    return Err(cancelled());
                }
                let discovered = discovery.discover(&input)?;
                let system = system.facts(scope);
                if input.cancelled.is_cancelled() {
                    return Err(cancelled());
                }
                Ok((discovered, system))
            });
            let (discovered, system) = tokio::select! {
                _ = source.cancelled.cancelled() => return Err(cancelled()),
                _ = request.cancelled.cancelled() => return Err(cancelled()),
                result = worker => result.map_err(|_| wire::AppError { code: wire::ErrorCode::NativeUnavailable,
                    message: "Owned read-only discovery worker stopped.".into(), retryable: false })??,
            };
            if source.cancelled.is_cancelled() || request.cancelled.is_cancelled() {
                return Err(cancelled());
            }
            let settings =
                adapter::persisted_settings(&source.settings).map_err(wire::AppError::from)?;
            let catalog = if legacy_sessions {
                let mut private = request.private_tabs.clone();
                private.extend(
                    source
                        .snapshot
                        .sessions
                        .iter()
                        .flat_map(|session| session.tabs.iter())
                        .filter(|tab| {
                            discovered.profiles.profiles.iter().any(|profile| {
                                profile.id == tab.profile_id.as_str() && profile.elevation.is_some()
                            })
                        })
                        .map(|tab| tab.id),
                );
                crate::facts::projection::project_legacy_catalog(
                    &source,
                    request.target_session,
                    &private,
                )?
            } else {
                Default::default()
            };
            // Independent provider errors stay missing, not guessed installed=false
            // or a fabricated home. Required v1 projections reject missing facts.
            let explorer = system.explorer.ok();
            let cli = system.cli.ok();
            let mut probe = discovered.probe;
            probe.capabilities.explorer_context_menu = explorer
                .as_ref()
                .is_some_and(|integration| integration.supported);
            Ok(ManagerFactsResult {
                facts: ManagerFacts {
                    settings: Some(SettingsFacts {
                        adapter_id: settings.adapter_id,
                        shell_id: settings.shell_id,
                        shell_executable: settings.shell_executable,
                        process_rules: settings.process_rules,
                        history_page_size: settings.history_page_size,
                        session_settle_hours: settings.session_settle_hours,
                        session_delete_hours: settings.session_delete_hours,
                        auto_update: settings.auto_update,
                        terminal_env: settings.terminal_env,
                    }),
                    probe: Some(probe),
                    explorer,
                    cli,
                    sessions: catalog.sessions,
                    omitted_sessions: catalog.omitted_sessions,
                },
                profiles: Some(discovered.profiles),
                home: system.home.ok(),
            })
        })
    }
}
impl SourceFactsProjector for NativeSourceFactsProjector {
    fn project(
        &self,
        source: Arc<SourceFacts>,
        request: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        self.project_mode(source, request, true)
    }
    fn project_capabilities(
        &self,
        source: Arc<SourceFacts>,
        request: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        self.project_mode(source, request, false)
    }
}
