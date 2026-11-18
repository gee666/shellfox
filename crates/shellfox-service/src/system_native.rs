use super::{
    cli,
    environment::{NativeSystemEnvironment, SystemEnvironment, SystemPlatform},
    error, explorer,
    io::{NativeSystemFiles, SystemFiles},
    linux,
    linux_discovery::{NativeSystemLinuxDiscovery, SystemLinuxDiscovery},
    opener::{NativeSystemOpener, SystemOpenTarget, SystemOpener},
    registry::SystemRegistry,
    resources::SystemInstallResources,
    SystemAction, SystemBackend, SystemResponse, SystemResult, SystemScope,
};
use shellfox_protocol::{CliIntegration, Directory, ErrorCode, ExplorerIntegration};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemInstallPolicy {
    pub allow_user_install: bool,
    pub allow_open: bool,
}

pub struct SystemProviders {
    pub environment: Arc<dyn SystemEnvironment>,
    pub resources: Arc<dyn SystemInstallResources>,
    pub files: Arc<dyn SystemFiles>,
    pub registry: Arc<dyn SystemRegistry>,
    pub opener: Arc<dyn SystemOpener>,
    pub linux_discovery: Arc<dyn SystemLinuxDiscovery>,
}

pub struct NativeSystemBackend {
    pub environment: Arc<dyn SystemEnvironment>,
    pub resources: Arc<dyn SystemInstallResources>,
    pub files: Arc<dyn SystemFiles>,
    pub registry: Arc<dyn SystemRegistry>,
    pub opener: Arc<dyn SystemOpener>,
    pub linux_discovery: Arc<dyn SystemLinuxDiscovery>,
    pub policy: SystemInstallPolicy,
    pub(super) mutations: Arc<Mutex<()>>,
}
impl NativeSystemBackend {
    pub fn new(providers: SystemProviders, policy: SystemInstallPolicy) -> Self {
        let SystemProviders {
            environment,
            resources,
            files,
            registry,
            opener,
            linux_discovery,
        } = providers;
        Self {
            environment,
            resources,
            files,
            registry,
            opener,
            linux_discovery,
            policy,
            mutations: Arc::new(Mutex::new(())),
        }
    }
    /// Only constructs providers. Does not discover/install anything at boot.
    pub fn native(
        resources: Arc<dyn SystemInstallResources>,
        policy: SystemInstallPolicy,
        interactive_session: bool,
    ) -> Self {
        let environment: Arc<dyn SystemEnvironment> = Arc::new(NativeSystemEnvironment);
        let files: Arc<dyn SystemFiles> = Arc::new(NativeSystemFiles);
        let discovery = Arc::new(NativeSystemLinuxDiscovery {
            environment: environment.clone(),
            files: files.clone(),
        });
        Self::new(
            SystemProviders {
                environment,
                resources,
                files,
                registry: Arc::new(super::NativeSystemRegistry),
                opener: Arc::new(NativeSystemOpener {
                    interactive_session,
                }),
                linux_discovery: discovery,
            },
            policy,
        )
    }
    fn install_allowed(&self, scope: SystemScope) -> SystemResult<()> {
        if scope == SystemScope::System {
            return Err(error(ErrorCode::Unsupported,"System installation requires a package manager/elevation policy that is not available; no UAC was attempted."));
        }
        if !self.policy.allow_user_install {
            return Err(error(
                ErrorCode::Unsupported,
                "Per-user installation policy is not enabled for this host.",
            ));
        }
        self.resources.validate_current()
    }
}
impl SystemBackend for NativeSystemBackend {
    fn home_directory(&self) -> SystemResult<Directory> {
        let path = self.environment.home()?;
        super::validation::absolute_path(&path)?;
        let cwd = path
            .to_str()
            .ok_or_else(|| {
                error(
                    ErrorCode::Validation,
                    "Host home path encoding is unsupported.",
                )
            })?
            .to_owned();
        Ok(Directory { cwd })
    }
    fn cli_integration(&self, scope: SystemScope) -> SystemResult<CliIntegration> {
        self.resources.validate_current()?;
        let plan = self.resources.cli_plan(scope)?;
        cli::query(
            &*self.files,
            &*self.registry,
            &*self.environment,
            &plan,
            scope,
        )
    }
    fn explorer_integration(&self, scope: SystemScope) -> SystemResult<ExplorerIntegration> {
        match self.environment.platform(){
            SystemPlatform::Windows=>{self.resources.validate_current()?;explorer::query(&*self.registry,&self.resources.desktop_launch()?,scope)},
            SystemPlatform::Linux=>{self.resources.validate_current()?;linux::query(&*self.files,&*self.registry,&*self.environment,&*self.linux_discovery,&self.resources.cli_plan(scope)?,&self.resources.desktop_launch()?,scope)},
            SystemPlatform::Macos=>Ok(ExplorerIntegration{supported:false,installed:false,folder_item_installed:false,background_installed:false,reason:Some("Finder context-menu installation is not supported by this application's platform integration.".into())}),
            SystemPlatform::Other=>Err(error(ErrorCode::Unsupported,"File-manager integration is unavailable on this platform.")),
        }
    }
    fn perform_user_action(&self, action: SystemAction) -> SystemResult<SystemResponse> {
        match action {
            SystemAction::OpenDirectory { path } => {
                if !self.policy.allow_open {
                    return Err(error(
                        ErrorCode::Unsupported,
                        "System opening is not enabled for this host.",
                    ));
                }
                let canonical = self.files.canonical_directory(&path)?;
                self.opener.open(SystemOpenTarget::Directory(canonical))?;
                Ok(SystemResponse::Opened)
            }
            SystemAction::OpenWeb { url } => {
                if !self.policy.allow_open {
                    return Err(error(
                        ErrorCode::Unsupported,
                        "Web opening is not enabled for this host.",
                    ));
                }
                let url = super::validation::web_url(&url)?;
                self.opener.open(SystemOpenTarget::Web(url))?;
                Ok(SystemResponse::Opened)
            }
            SystemAction::SetCli { installed, scope } => {
                let _serial = self.mutations.lock().map_err(|_| {
                    error(
                        ErrorCode::NativeUnavailable,
                        "Installation state requires recovery.",
                    )
                })?;
                self.install_allowed(scope)?;
                let plan = self.resources.cli_plan(scope)?;
                Ok(SystemResponse::Cli(cli::set(
                    &*self.files,
                    &*self.registry,
                    &*self.environment,
                    &plan,
                    scope,
                    installed,
                )?))
            }
            SystemAction::SetExplorer { installed, scope } => {
                let _serial = self.mutations.lock().map_err(|_| {
                    error(
                        ErrorCode::NativeUnavailable,
                        "Installation state requires recovery.",
                    )
                })?;
                self.install_allowed(scope)?;
                let launch = self.resources.desktop_launch()?;
                let value = match self.environment.platform() {
                    SystemPlatform::Windows => {
                        explorer::set(&*self.registry, &launch, scope, installed)?
                    }
                    SystemPlatform::Linux => linux::set(
                        &*self.files,
                        &*self.registry,
                        &*self.environment,
                        &*self.linux_discovery,
                        &self.resources.cli_plan(scope)?,
                        &launch,
                        (scope, installed),
                    )?,
                    _ => {
                        return Err(error(
                            ErrorCode::Unsupported,
                            "File-manager installation is not supported on this host.",
                        ))
                    }
                };
                Ok(SystemResponse::Explorer(value))
            }
        }
    }
}
