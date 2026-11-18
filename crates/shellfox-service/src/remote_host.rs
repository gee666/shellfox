//! Ready facade assembled by the retained, borrowed native preparation owner.
use super::preferences::store::NativePreferenceStore;
use shellfox_remote::{
    owner_channel, ApprovalControl, Backend, BackendHandle, PrivateApprovalStore,
};
use shellfox_transport::{
    HelperConfig, IsolatedStateDirectory, LoopbackOrigin, Secret, VerifiedHelper,
};
use std::sync::Arc;
pub struct RemoteBackend {
    backend: BackendHandle,
    engine: Arc<Backend>,
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
    ) -> anyhow::Result<(shellfox_remote::OwnerReceiver, Self)> {
        let secret = fresh_secret()?;
        let (sender, receiver) = owner_channel(32);
        let engine = Backend::new(Arc::new(sender), approvals.clone(), secret.clone());
        #[cfg(feature = "experimental-shellfox-native")]
        let engine = {
            // Existing Runtime verifier supplies the actual local sender. No
            // request metadata or guessed identity may prepare this capability.
            let verified = shellfox_runtime::Runtime::native0_engine_identity()?;
            let identity = String::from_utf8(verified.as_bytes().to_vec())?;
            let codec = shellfox_protocol::manager_v2::Native0Codec::new(
                shellfox_protocol::manager_v2::EngineIdentity::try_from(identity)
                    .map_err(|_| anyhow::anyhow!("Verified manager engine identity is invalid"))?,
            );
            engine
                .prepare_manager_v2_native0(codec)
                .map_err(|_| anyhow::anyhow!("Verified manager preparation failed"))?
        };
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
            engine: self.engine,
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
    pub async fn observer_v2_signed(
        &self,
        endpoint: shellfox_protocol::Endpoint,
        body: &[u8],
        headers: shellfox_transport::incoming::SignedHeaders<'_>,
    ) -> shellfox_remote::ObserverHttpReply {
        self.engine
            .handle_observer_v2_signed(endpoint, body, headers)
            .await
    }
    pub async fn manager_v2_signed(
        &self,
        endpoint: shellfox_protocol::Endpoint,
        body: &[u8],
        headers: shellfox_transport::incoming::SignedHeaders<'_>,
    ) -> shellfox_remote::ManagerHttpReply {
        self.engine
            .handle_manager_v2_signed(endpoint, body, headers)
            .await
    }
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
