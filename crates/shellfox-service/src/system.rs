//! Host-only OS integration. Queries are synchronous and read-only; callers
//! schedule them off the owner/UI thread. Explicit actions use a separate path.
//!
//! References: main/platform/{shellfox-cli,linux-cli,linux-menus,explorer}.ts,
//! directory-prompt.ts and desktop-requests.ts. Query paths never reproduce the
//! legacy Explorer.get migration/write side effects. Installation resources are
//! supplied by the validated package composition root, not current_exe().
//! Native system-wide mutation/elevation is deliberately unavailable. URI syntax
//! is not authorization. Registry and file revision checks are cooperative edit
//! conflict detection, not a hostile same-user namespace immutability claim.
#[cfg(test)]
mod tests;
use shellfox_protocol::{AppError, CliIntegration, Directory, ExplorerIntegration, Validate};
use std::sync::Arc;
#[cfg(test)]
pub(crate) use tests::support::Fixture as OwnedSystemFixture;

mod types;
pub use types::{
    SystemAction, SystemBackend, SystemFacts, SystemResponse, SystemResult, SystemScope,
};
mod cli;
mod deb;
mod environment;
mod explorer;
mod file_platform;
mod io;
mod linux;
mod linux_discovery;
mod linux_templates;
mod native;
mod native_api;
mod native_authority;
mod native_capabilities;
mod native_guard;
mod native_handler;
mod native_service;
mod opener;
pub use native_api::{
    SystemCapability, SystemMutationCapabilities, SystemNativeAuthority, SystemNativeCapabilities,
    SystemNativeRequest, SystemNativeResponse, SystemNativeStatus,
};
pub use native_handler::SystemNativeHandler;
mod registry;
mod resources;
mod thunar;
mod validation;
#[cfg(windows)]
mod windows_registry;
pub use deb::DebPackage;
pub use environment::{NativeSystemEnvironment, SystemEnvironment, SystemPlatform};
#[cfg(windows)]
pub(crate) use io::file_revision;
pub use io::{FileRevision, NativeSystemFiles, SystemFile, SystemFiles};
pub use linux_discovery::{
    FileManager, LinuxMenuDiscovery, NativeSystemLinuxDiscovery, SystemLinuxDiscovery,
};
pub use native::{NativeSystemBackend, SystemInstallPolicy, SystemProviders};
pub use opener::{NativeSystemOpener, SystemOpenTarget, SystemOpener};
#[cfg(not(windows))]
pub use registry::NativeSystemRegistry;
pub use registry::{
    RegistryField, RegistryRevision, RegistrySlot, RegistryState, RegistryStringKind, RegistryText,
    SystemRegistry,
};
pub use resources::{
    validate_stable_package_root, CliInstallPlan, ConfiguredSystemResources, DesktopLaunch,
    DesktopLaunchDisposition, ManagedFile, SystemInstallResources,
};
pub use validation::{absolute_path as validate_system_path, web_url as validate_web_url};
#[cfg(windows)]
pub use windows_registry::NativeSystemRegistry;

pub struct HostSystemServices {
    backend: Arc<dyn SystemBackend>,
}
impl HostSystemServices {
    pub fn new(backend: Arc<dyn SystemBackend>) -> Self {
        Self { backend }
    }
    pub fn home_directory(&self) -> SystemResult<Directory> {
        let value = self.backend.home_directory()?;
        value.validate().map_err(|_| invalid_provider())?;
        Ok(value)
    }
    pub fn cli_integration(&self, scope: SystemScope) -> SystemResult<CliIntegration> {
        let value = self.backend.cli_integration(scope)?;
        if !matches!(value.command.as_str(), "shellfox" | "shellfox start <path>")
            || value.installed && !value.supported
            || !valid_reason(value.reason.as_deref())
        {
            return Err(invalid_provider());
        }
        Ok(value)
    }
    pub fn explorer_integration(&self, scope: SystemScope) -> SystemResult<ExplorerIntegration> {
        let value = self.backend.explorer_integration(scope)?;
        if value.installed != (value.folder_item_installed && value.background_installed)
            || (value.folder_item_installed || value.background_installed) && !value.supported
            || !valid_reason(value.reason.as_deref())
        {
            return Err(invalid_provider());
        }
        Ok(value)
    }
    pub fn facts(&self, scope: SystemScope) -> SystemFacts {
        SystemFacts {
            home: self.home_directory(),
            cli: self.cli_integration(scope),
            explorer: self.explorer_integration(scope),
        }
    }
    /// 0.9's always-on repair: installs the CLI when it is missing. Never removes.
    pub fn ensure_cli(&self) -> SystemResult<CliIntegration> {
        let current = self.cli_integration(SystemScope::User)?;
        if current.installed {
            return Ok(current);
        }
        let action = SystemAction::SetCli {
            installed: true,
            scope: SystemScope::User,
        };
        match self.perform_user_action(action)? {
            SystemResponse::Cli(value) => Ok(value),
            _ => Err(invalid_provider()),
        }
    }
    /// Installs the file-manager menus when supported and missing. Never removes.
    pub fn ensure_explorer(&self) -> SystemResult<ExplorerIntegration> {
        let current = self.explorer_integration(SystemScope::User)?;
        if !current.supported {
            let reason = current.reason.as_deref();
            return Err(error(
                shellfox_protocol::ErrorCode::Unsupported,
                reason.unwrap_or("This integration is unsupported."),
            ));
        }
        if current.installed {
            return Ok(current);
        }
        let action = SystemAction::SetExplorer {
            installed: true,
            scope: SystemScope::User,
        };
        match self.perform_user_action(action)? {
            SystemResponse::Explorer(value) => Ok(value),
            _ => Err(invalid_provider()),
        }
    }
    /// Routing must authorize an explicit local user action before this call.
    /// A parsed URI or supplied install path is never authorization by itself.
    pub fn perform_user_action(&self, action: SystemAction) -> SystemResult<SystemResponse> {
        action.validate()?;
        self.backend.perform_user_action(action)
    }
}
fn valid_reason(reason: Option<&str>) -> bool {
    reason.is_none_or(|reason| reason.len() <= 1000 && !reason.contains('\0'))
}
pub(super) fn error(code: shellfox_protocol::ErrorCode, message: &str) -> AppError {
    AppError {
        code,
        message: message.into(),
        retryable: false,
    }
}
fn invalid_provider() -> AppError {
    AppError {
        code: shellfox_protocol::ErrorCode::NativeUnavailable,
        message: "System integration provider returned invalid facts.".into(),
        retryable: false,
    }
}
