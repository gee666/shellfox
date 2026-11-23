//! Ready facade assembled by the retained, borrowed native preparation owner.
use super::preferences::store::NativePreferenceStore;
use shellfox_remote::{
    ApprovalControl, Backend, BackendHandle, PrivateApprovalStore,
};
use shellfox_transport::{
    HelperConfig, IsolatedStateDirectory, LoopbackOrigin, Secret, VerifiedHelper,
};
use std::sync::Arc;
pub struct RemoteBackend {
    backend: BackendHandle,
    approvals: ApprovalControl,
    store: Arc<PrivateApprovalStore>,
    origin: LoopbackOrigin,
    secret: Secret,
    helper: VerifiedHelper,
    pub(super) preferences: Arc<NativePreferenceStore>,
}
/// Stored in RemoteHostSetup BEFORE binding awaits. No path-based persistence.
pub(super) struct BackendAssembly {
    pub engine: Arc<Backend>,
    approvals: ApprovalControl,
    store: Arc<PrivateApprovalStore>,
    secret: Secret,
    helper: VerifiedHelper,
    preferences: Arc<NativePreferenceStore>,
}
impl BackendAssembly {
    pub fn new(
        helper: VerifiedHelper,
        approvals: ApprovalControl,
        store: Arc<PrivateApprovalStore>,
        preferences: Arc<NativePreferenceStore>,
    ) -> anyhow::Result<(super::RemoteInbox, Self)> {
        let secret = fresh_secret()?;
        let (sender, receiver) = super::remote_channel();
        let engine = Backend::new(Arc::new(sender), approvals.clone(), secret.clone());
        let engine = Arc::new(engine);
        Ok((
            receiver,
            Self {
                engine,
                approvals,
                store,
                secret,
                helper,
                preferences,
            },
        ))
    }
    pub fn finish(self, origin: LoopbackOrigin, backend: BackendHandle) -> RemoteBackend {
        RemoteBackend {
            backend,
            approvals: self.approvals,
            store: self.store,
            origin,
            secret: self.secret,
            helper: self.helper,
            preferences: self.preferences,
        }
    }
}
impl RemoteBackend {
    pub fn approvals(&self) -> ApprovalControl {
        self.approvals.clone()
    }
    pub fn approval_store(&self) -> Arc<PrivateApprovalStore> {
        self.store.clone()
    }
    /// Go library path-based state/config is separate and NOT anchored/accepted
    /// by the native Rust metadata capability used for preferences/approvals.
    pub fn helper_config(
        &self,
        state_directory: IsolatedStateDirectory,
        hostname: String,
    ) -> anyhow::Result<HelperConfig> {
        Ok(HelperConfig {
            executable: self.helper.clone(),
            state_directory,
            hostname,
            backend: self.origin.clone(),
            backend_secret: self.secret.clone(),
            proxy_secret: fresh_secret()?,
        })
    }
    pub async fn shutdown(self) {
        self.backend.shutdown().await;
    }
}
fn fresh_secret() -> anyhow::Result<Secret> {
    Secret::new(format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    ))
    .map_err(Into::into)
}
