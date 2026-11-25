use super::{
    error, native_capabilities,
    native_guard::{GuardedSystemFiles, GuardedSystemResources, SystemNativeGuard},
    HostSystemServices, NativeSystemBackend, SystemAction, SystemCapability, SystemNativeAuthority,
    SystemNativeRequest, SystemNativeResponse, SystemNativeStatus, SystemProviders, SystemResponse,
    SystemResult, SystemScope,
};
use shellfox_protocol::{CancellationToken, ErrorCode};
use std::sync::Arc;

/// Real host provider adapter. No default backend/resources, chooser, command
/// runner or remote request conversion exists. Native owner admission is separate.
pub struct SystemNativeHandler {
    backend: Arc<NativeSystemBackend>,
    services: Arc<HostSystemServices>,
    guard: SystemNativeGuard,
}
impl SystemNativeHandler {
    /// Use the FINAL owning host's loss token. Queries retain typed unavailable
    /// installation facts when no stable package is configured. Mutation
    /// capabilities and each install action still require validated resources.
    /// This performs no installation or package discovery.
    pub fn new(
        backend: Arc<NativeSystemBackend>,
        ownership_lost: CancellationToken,
    ) -> SystemResult<Self> {
        let guard = SystemNativeGuard {
            lost: ownership_lost,
            admissions: Arc::new(super::native_authority::SystemNativeAdmissions::new()),
        };
        guard.check()?;
        let mut scoped = NativeSystemBackend::new(
            SystemProviders {
                environment: backend.environment.clone(),
                resources: Arc::new(GuardedSystemResources {
                    inner: backend.resources.clone(),
                    guard: guard.clone(),
                }),
                files: Arc::new(GuardedSystemFiles {
                    inner: backend.files.clone(),
                    guard: guard.clone(),
                }),
                opener: backend.opener.clone(),
                linux_discovery: backend.linux_discovery.clone(),
            },
            backend.policy,
        );
        // Share the real provider's mutation serialization, not a parallel
        // installation state machine or a fresh uncoordinated writer lock.
        scoped.mutations = backend.mutations.clone();
        let backend = Arc::new(scoped);
        let services = Arc::new(HostSystemServices::new(backend.clone()));
        guard.check()?;
        Ok(Self {
            backend,
            services,
            guard,
        })
    }
    /// Whether this host may open folders and links: a desktop-owned host.
    pub(crate) fn desktop_actions(&self) -> bool {
        self.backend.policy.allow_open
    }
    /// All methods are synchronous. Execute under the original local/native
    /// owner authority; never call from a remote wire dispatcher or mobile API.
    /// External writes are not retried if ownership/reply is lost after admission.
    pub fn handle(
        &self,
        controller: u64,
        request: SystemNativeRequest,
        authority: &dyn SystemNativeAuthority,
    ) -> SystemResult<SystemNativeResponse> {
        self.check(controller, authority)?;
        request.validate()?;
        let scope = request.scope();
        let status = self.status(scope)?;
        self.check(controller, authority)?;
        let (action, wants_cli) = match request {
            SystemNativeRequest::Status { .. } => {
                return Ok(SystemNativeResponse::Status(Box::new(status)))
            }
            SystemNativeRequest::SetCli { installed, .. } => {
                require(if installed {
                    &status.capabilities.cli.install
                } else {
                    &status.capabilities.cli.uninstall
                })?;
                (SystemAction::SetCli { installed, scope }, true)
            }
            SystemNativeRequest::SetExplorer { installed, .. } => {
                require(if installed {
                    &status.capabilities.explorer.install
                } else {
                    &status.capabilities.explorer.uninstall
                })?;
                (SystemAction::SetExplorer { installed, scope }, false)
            }
        };
        self.check(controller, authority)?;
        let response = self.services.perform_user_action(action);
        self.check(controller, authority)?;
        match response? {
            SystemResponse::Cli(value) if wants_cli => Ok(SystemNativeResponse::Cli(value)),
            SystemResponse::Explorer(value) if !wants_cli => {
                Ok(SystemNativeResponse::Explorer(value))
            }
            _ => Err(error(
                ErrorCode::Internal,
                "System provider returned the wrong action response.",
            )),
        }
    }
    pub(crate) fn admit_owner(&self, controller: u64) -> SystemResult<()> {
        self.guard.check()?;
        self.guard.admissions.admit(controller)
    }
    pub(crate) fn validate_owner(&self, controller: u64) -> SystemResult<()> {
        self.guard.check()?;
        self.guard.admissions.validate(controller)
    }
    pub(crate) fn revoke_owner(&self, controller: u64) {
        self.guard.admissions.revoke(controller)
    }
    pub(crate) fn begin_stop(&self) {
        self.guard.admissions.begin_stop()
    }
    fn status(&self, scope: SystemScope) -> SystemResult<SystemNativeStatus> {
        self.guard.check()?;
        let facts = self.services.facts(scope);
        self.guard.check()?;
        let capabilities = native_capabilities::collect(&self.backend, scope, &facts);
        self.guard.check()?;
        Ok(SystemNativeStatus {
            scope,
            facts,
            capabilities,
        })
    }
    fn check(&self, controller: u64, authority: &dyn SystemNativeAuthority) -> SystemResult<()> {
        self.guard.check()?;
        if controller == 0 {
            return Err(error(
                ErrorCode::AuthFailed,
                "An active original local/native controller is required.",
            ));
        }
        authority.validate_system_controller(controller)?;
        self.guard.check()
    }
}
fn require(capability: &SystemCapability) -> SystemResult<()> {
    match capability {
        SystemCapability::Available => Ok(()),
        SystemCapability::Unavailable(error) => Err(error.clone()),
    }
}
