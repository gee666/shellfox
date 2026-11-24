//! Shared Desktop/CLI application composition, not another Host or persistence owner.
use crate::{
    local::RemoteBootOptions,
    native_providers::NativeHostProviders,
    remote::{ControlServerOrigin, RemoteBootResources},
    system::{
        self, CliInstallPlan, DesktopLaunch, NativeSystemBackend, SystemInstallPolicy,
        SystemInstallResources, SystemResult, SystemScope,
    },
};
use anyhow::{bail, ensure, Context, Result};
use shellfox_protocol::{AppError, ErrorCode};
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
pub struct ApplicationArguments {
    state: Option<PathBuf>,
    hostname: Option<String>,
    control_server: Option<ControlServerOrigin>,
}
impl ApplicationArguments {
    pub fn is_flag(flag: &std::ffi::OsStr) -> bool {
        matches!(
            flag.to_str(),
            Some("--remote-state-dir" | "--remote-hostname" | "--remote-control-url")
        )
    }
    pub fn push(&mut self, flag: &str, value: OsString) -> Result<()> {
        match flag {
            "--remote-state-dir" => {
                ensure!(self.state.is_none(), "Duplicate --remote-state-dir");
                self.state = Some(value.into());
            }
            "--remote-hostname" => {
                ensure!(self.hostname.is_none(), "Duplicate --remote-hostname");
                self.hostname = Some(
                    value
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("Remote hostname must be UTF-8"))?,
                );
            }
            "--remote-control-url" => {
                ensure!(
                    self.control_server.is_none(),
                    "Duplicate --remote-control-url"
                );
                self.control_server = Some(
                    ControlServerOrigin::parse_control_url(
                        value.to_str().context("Control URL must be UTF-8")?,
                    )
                    .map_err(|error| anyhow::anyhow!(error))?,
                );
            }
            _ => bail!("Unknown application composition flag"),
        }
        Ok(())
    }
    pub fn finish(self, data: &Path) -> Result<NativeApplicationConfig> {
        let mut config = NativeApplicationConfig::new(data, self.state, self.hostname)?;
        config.control_server = self.control_server;
        Ok(config)
    }
}

#[derive(Clone, Debug)]
pub struct NativeApplicationConfig {
    pub remote_state_dir: PathBuf,
    pub hostname: String,
    pub control_server: Option<ControlServerOrigin>,
}
impl NativeApplicationConfig {
    /// Planning-only: explicit application namespace, never production/home/cwd discovery.
    pub fn new(data: &Path, state: Option<PathBuf>, hostname: Option<String>) -> Result<Self> {
        // Canonical native control namespace supplied by the Service composition.
        // This is descriptive only; RemoteHostSetup alone materializes it after its gate.
        let network = data.join("rust-service-v1").join("go-network");
        let state = state.unwrap_or_else(|| crate::app_paths::default_remote_state(data));
        system::validate_system_path(&state).map_err(|error| anyhow::anyhow!(error.message))?;
        ensure!(
            !state.components().any(|part| part == Component::ParentDir),
            "Remote state path cannot contain parent traversal"
        );
        let suffix = state.strip_prefix(&network).context(
            "Remote state must stay under this Host's rust-service-v1/go-network namespace",
        )?;
        ensure!(
            (1..=15).contains(&suffix.components().count())
                && suffix
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "Remote state requires a bounded private leaf below go-network"
        );
        let hostname = match hostname {
            Some(name) => name,
            None => match RemoteBootResources::planned_for_current_host(state.clone())? {
                RemoteBootResources::Planned { hostname, .. } => hostname,
                RemoteBootResources::Prepared(_) => {
                    bail!("OS hostname planning returned prepared resources")
                }
            },
        };
        ensure!(
            !hostname.is_empty()
                && hostname.len() <= 63
                && hostname
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !hostname.starts_with('-')
                && !hostname.ends_with('-'),
            "Remote hostname must be a 1-63 character DNS label"
        );
        Ok(Self {
            remote_state_dir: state,
            hostname,
            control_server: None,
        })
    }
    pub fn validate_separate(&self, roots: &[&Path]) -> Result<()> {
        for root in roots {
            ensure!(
                !lexical_overlap(&self.remote_state_dir, root),
                "Remote state must not overlap frontend namespaces"
            );
        }
        Ok(())
    }
    pub fn remote_options(
        &self,
        supervisor: PathBuf,
        interactive: bool,
    ) -> Result<RemoteBootOptions> {
        let resources = match &self.control_server {
            Some(control) => RemoteBootResources::planned_with_control_server(
                self.remote_state_dir.clone(),
                self.hostname.clone(),
                control.clone(),
            )?,
            None => {
                RemoteBootResources::planned(self.remote_state_dir.clone(), self.hostname.clone())?
            }
        };
        let providers = unconfigured_providers(interactive);
        RemoteBootOptions::native(
            supervisor,
            providers.system_services(),
            SystemScope::User,
            resources,
        )?
        .with_native_providers(providers)
    }
}
// Early lexical rejection only. Native preparation retains/revalidates actual path identities.
fn lexical_overlap(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    let (left, right) = (
        PathBuf::from(left.to_string_lossy().to_lowercase()),
        PathBuf::from(right.to_string_lossy().to_lowercase()),
    );
    #[cfg(not(windows))]
    let (left, right) = (left.to_path_buf(), right.to_path_buf());
    left.starts_with(&right) || right.starts_with(&left)
}
struct UnconfiguredInstallResources;
fn unavailable<T>() -> SystemResult<T> {
    Err(AppError {
        code: ErrorCode::NativeUnavailable,
        message:
            "Stable package installation resources are not configured for this application launch."
                .into(),
        retryable: false,
    })
}
impl SystemInstallResources for UnconfiguredInstallResources {
    fn cli_plan(&self, _scope: SystemScope) -> SystemResult<CliInstallPlan> {
        unavailable()
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        unavailable()
    }
    fn validate_current(&self) -> SystemResult<()> {
        unavailable()
    }
}
fn unconfigured_providers(interactive: bool) -> NativeHostProviders {
    // Standalone launches can use host System actions but cannot install
    // integrations without a validated packaged launch.
    NativeHostProviders::new(Arc::new(NativeSystemBackend::native(
        Arc::new(UnconfiguredInstallResources),
        SystemInstallPolicy {
            allow_open: interactive,
            ..SystemInstallPolicy::default()
        },
        interactive,
    )))
}
/// System services of a standalone launch without packaged install resources.
pub fn system_services(interactive: bool) -> Arc<system::HostSystemServices> {
    unconfigured_providers(interactive).system_services()
}

#[cfg(test)]
mod tests;
