//! The owned boot entrypoint. It binds signed loopback offline, not Tailscale.
#[cfg(test)]
mod tests;
use super::*;
use shellfox_transport::{AcquiredOwnershipView, PermissionVerifier, VerifiedHelper};
use std::sync::Arc;
mod resources;
pub use resources::{RemoteBootResources, RemoteResources};
/// Stored by the original BootGuard BEFORE borrowed fallible initialization.
#[must_use = "retain this envelope through confirmed shutdown and private disposition"]
pub struct RemoteHostSetup {
    _control: crate::local::NativeControlChild,
    lost: wire::CancellationToken,
    preferences: Arc<preferences::store::NativePreferenceStore>,
    store: Arc<shellfox_remote::PrivateApprovalStore>,
    assembly: Option<host::BackendAssembly>,
    bound: Option<(
        shellfox_transport::LoopbackOrigin,
        shellfox_remote::BackendHandle,
    )>,
    backend: Option<Arc<RemoteBackend>>,
    runtime: Option<runtime::RemoteRuntime>,
    dispatcher: Arc<crate::owner::approvals::RemoteInstall>,
    attempted: bool,
    resources: Option<RemoteResources>,
    planned_state: Option<std::path::PathBuf>,
    network_parent: Option<Arc<shellfox_files::FileService>>,
    state_attempted: bool,
}
impl RemoteHostSetup {
    /// Infallible capability/lifetime retention only; no filesystem or network IO.
    pub(crate) fn retained(
        control: crate::local::NativeControlChild,
        ownership: &AcquiredOwnershipView<'_>,
    ) -> Self {
        let files = control.files();
        let lost = ownership.lost();
        let preferences = Arc::new(preferences::store::NativePreferenceStore::retained(
            files.clone(),
            lost.clone(),
        ));
        let store = Arc::new(shellfox_remote::PrivateApprovalStore::retained_native(
            files,
            lost.clone(),
        ));
        Self {
            _control: control,
            lost,
            preferences,
            store,
            assembly: None,
            bound: None,
            backend: None,
            runtime: None,
            dispatcher: crate::owner::approvals::RemoteInstall::new(),
            attempted: false,
            resources: None,
            planned_state: None,
            network_parent: None,
            state_attempted: false,
        }
    }
    pub(crate) fn bind_repository(
        &self,
        repository: Arc<dyn shellfox_remote::ApprovalRepository>,
    ) -> Result<(), AppError> {
        self.store.bind_repository(repository)
    }
    pub(crate) async fn install_dispatcher(
        &self,
        owner: &crate::owner::ServiceOwner,
        closing: &shellfox_protocol::CancellationToken,
    ) -> anyhow::Result<()> {
        self.dispatcher.install(owner, closing).await
    }
    /// Borrowed, single attempt. The caller has ALREADY stored this envelope.
    /// Go library state/config is still path-based and separately unaccepted.
    pub(crate) async fn initialize(
        &mut self,
        helper: &VerifiedHelper,
        facts: Arc<dyn ManagerFactsProvider>,
        resources: RemoteBootResources,
        control_path: &std::path::Path,
        verifier: &PermissionVerifier,
    ) -> Result<RemoteControl, AppError> {
        if self.attempted || self.lost.is_cancelled() {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "Native remote preparation is unavailable; no retry was made.",
            ));
        }
        self.attempted = true;
        // Operational/authentication failures remain errors with retained
        // custody; they never become a fabricated LocalOnly success.
        self.prepare_resources(resources, control_path, verifier)
            .await?;
        let resources = self
            .resources
            .as_ref()
            .ok_or_else(|| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Original network state proof is missing.",
                )
            })?
            .clone();
        self.store.initialize_read_only()?;
        let approvals = shellfox_remote::ApprovalControl::load(&self.store).await?;
        if self.lost.is_cancelled() {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "Native ownership lost before remote assembly.",
            ));
        }
        let (receiver, assembly) = host::BackendAssembly::new(
            helper.clone(),
            approvals,
            self.store.clone(),
            self.preferences.clone(),
        )
        .map_err(|_| failure(ErrorCode::NativeUnavailable, "Remote assembly unavailable."))?;
        self.assembly = Some(assembly);
        let engine = self
            .assembly
            .as_ref()
            .ok_or_else(|| failure(ErrorCode::Internal, "Retained remote assembly missing."))?
            .engine
            .clone();
        let bound = engine
            .bind_until(self.lost.clone())
            .await
            .map_err(|_| failure(ErrorCode::NativeUnavailable, "Owned loopback bind failed."))?;
        self.bound = Some(bound);
        if self.lost.is_cancelled() {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "Native ownership lost after remote bind; cleanup retained.",
            ));
        }
        // No fallible operation after taking partial resources until ready facade is retained.
        let assembly = self
            .assembly
            .take()
            .ok_or_else(|| failure(ErrorCode::Internal, "Retained assembly missing."))?;
        let (origin, bound) = self
            .bound
            .take()
            .ok_or_else(|| failure(ErrorCode::Internal, "Retained listener missing."))?;
        self.backend = Some(Arc::new(assembly.finish(origin, bound)));
        let backend = self
            .backend
            .as_ref()
            .ok_or_else(|| failure(ErrorCode::Internal, "Ready backend missing."))?
            .clone();
        let mut state = RemoteState::new(Default::default(), None, None);
        match shellfox_remote::RemoteTransfers::open(Default::default()) {
            Ok(transfers) => state.transfers = Some(transfers),
            Err(error) => eprintln!("Remote file transfers unavailable: {}", error.message),
        }
        // All native/backend custody remains in this original envelope while
        // the unique dispatcher is boxed. Retain it BEFORE the next await.
        self.dispatcher
            .prepare(Box::new(
                RemoteDispatcher::new(receiver, state)
                    .with_facts_provider(facts, tokio::runtime::Handle::current()),
            ))
            .map_err(|_| {
                failure(
                    ErrorCode::NativeUnavailable,
                    "Original remote dispatcher retention failed.",
                )
            })?;
        let (control, runtime) = runtime::RemoteRuntime::spawn(
            backend,
            resources.state_directory,
            resources.network_factory,
            resources.hostname,
            self.lost.clone(),
        )
        .await?;
        self.runtime = Some(runtime);
        if self.lost.is_cancelled() {
            return Err(failure(
                ErrorCode::NativeUnavailable,
                "Native ownership lost during remote initialization; cleanup retained.",
            ));
        }
        Ok(control)
    }
    pub async fn shutdown(&mut self) -> Result<(), AppError> {
        if let Some(runtime) = &mut self.runtime {
            runtime.shutdown().await?;
            self.runtime = None;
        }
        if let Some(backend) = self.backend.take() {
            match Arc::try_unwrap(backend) {
                Ok(backend) => backend.shutdown().await,
                Err(backend) => {
                    self.backend = Some(backend);
                    return Err(failure(
                        ErrorCode::NativeUnavailable,
                        "CleanupRetained: remote backend still retained.",
                    ));
                }
            }
        }
        if let Some((_origin, backend)) = self.bound.take() {
            backend.shutdown().await;
        }
        self.assembly = None;
        Ok(())
    }
}
