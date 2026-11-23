#[cfg(test)]
mod tests;
use super::*;
pub type NetworkFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, AppError>> + Send + 'a>>;
pub trait RemoteNetwork: OutboundTransport {
    /// Status-preserving transport is explicit; never manufacture a status from
    /// a legacy send error. Old injected adapters deny until implemented.
    fn request_v2_with_status<'a>(
        &'a self,
        _peer: &'a wire::AuthenticatedPeerId,
        _endpoint: crate::remote::OutgoingV2Endpoint,
        _body: Vec<u8>,
        _limits: wire::RequestLimits,
    ) -> crate::remote::OutgoingV2Future<'a> {
        Box::pin(async { Err(wire::TransportError::Unavailable) })
    }
    fn status(&self) -> NetworkFuture<'_, NetworkStatus>;
    fn peers(&self) -> NetworkFuture<'_, Vec<NetworkPeer>>;
    fn login(&self, key: Option<AuthKey>) -> NetworkFuture<'_, NetworkStatus>;
    /// Ok requires original owned process exit confirmation. On error retain
    /// the controller for retry; ACK, EOF and cancellation are not exit proof.
    fn shutdown(&self) -> NetworkFuture<'_, ()>;
}
pub trait RemoteControlBackend: Send + Sync {
    fn preferences(&self) -> NetworkFuture<'_, Option<super::preferences::RemotePreferences>>;
    fn save_preferences(
        &self,
        value: super::preferences::RemotePreferences,
    ) -> NetworkFuture<'_, ()>;
    fn approvals(&self) -> shellfox_remote::ApprovalControl;
    fn helper_config(
        &self,
        directory: IsolatedStateDirectory,
        hostname: String,
    ) -> Result<HelperConfig, AppError>;
    fn approve_full_account<'a>(&'a self, peer_id: &'a str) -> NetworkFuture<'a, ()>;
    fn revoke<'a>(&'a self, peer: &'a str) -> NetworkFuture<'a, ()>;
}
impl RemoteControlBackend for RemoteBackend {
    fn preferences(&self) -> NetworkFuture<'_, Option<super::preferences::RemotePreferences>> {
        Box::pin(self.read_preferences())
    }
    fn save_preferences(
        &self,
        value: super::preferences::RemotePreferences,
    ) -> NetworkFuture<'_, ()> {
        Box::pin(self.write_preferences(value))
    }
    fn approvals(&self) -> shellfox_remote::ApprovalControl {
        RemoteBackend::approvals(self)
    }
    fn helper_config(
        &self,
        directory: IsolatedStateDirectory,
        hostname: String,
    ) -> Result<HelperConfig, AppError> {
        RemoteBackend::helper_config(self, directory, hostname).map_err(|_| {
            error(
                ErrorCode::NativeUnavailable,
                "Helper configuration unavailable.",
            )
        })
    }
    fn approve_full_account<'a>(&'a self, peer_id: &'a str) -> NetworkFuture<'a, ()> {
        Box::pin(async move {
            self.approvals()
                .approve_full_account(&self.approval_store(), peer_id)
                .await
        })
    }
    fn revoke<'a>(&'a self, peer: &'a str) -> NetworkFuture<'a, ()> {
        Box::pin(async move { self.approvals().revoke(&self.approval_store(), peer).await })
    }
}
pub trait RemoteNetworkFactory: Send + Sync {
    fn start(&self, config: HelperConfig) -> NetworkFuture<'_, Arc<dyn RemoteNetwork>>;
}
pub struct GoNetworkFactory;
struct GoControlNetworkFactory(shellfox_transport::LoopbackOrigin);
impl GoNetworkFactory {
    /// Immutable boot-time operational setting, never a peer request URL.
    pub fn with_control_server(
        control: shellfox_transport::LoopbackOrigin,
    ) -> Arc<dyn RemoteNetworkFactory> {
        Arc::new(GoControlNetworkFactory(control))
    }
}
struct GoNetwork(HelperTransport);
impl RemoteNetworkFactory for GoControlNetworkFactory {
    fn start(&self, config: HelperConfig) -> NetworkFuture<'_, Arc<dyn RemoteNetwork>> {
        let control = self.0.clone();
        Box::pin(async move {
            let helper = HelperTransport::start_with_control_server(config, control)
                .await
                .map_err(helper_error)?;
            Ok(Arc::new(GoNetwork(helper)) as Arc<dyn RemoteNetwork>)
        })
    }
}
impl RemoteNetworkFactory for GoNetworkFactory {
    fn start(&self, config: HelperConfig) -> NetworkFuture<'_, Arc<dyn RemoteNetwork>> {
        Box::pin(async move {
            let helper = HelperTransport::start(config).await.map_err(helper_error)?;
            Ok(Arc::new(GoNetwork(helper)) as Arc<dyn RemoteNetwork>)
        })
    }
}
impl OutboundTransport for GoNetwork {
    fn send<'a>(
        &'a self,
        peer: &'a wire::AuthenticatedPeerId,
        endpoint: wire::Endpoint,
        body: Vec<u8>,
        limits: wire::RequestLimits,
    ) -> wire::TransportFuture<'a> {
        self.0.send(peer, endpoint, body, limits)
    }
}
impl RemoteNetwork for GoNetwork {
    fn request_v2_with_status<'a>(
        &'a self,
        peer: &'a wire::AuthenticatedPeerId,
        endpoint: crate::remote::OutgoingV2Endpoint,
        body: Vec<u8>,
        limits: wire::RequestLimits,
    ) -> crate::remote::OutgoingV2Future<'a> {
        Box::pin(
            self.0
                .request_with_status(peer, endpoint.wire(), body, limits),
        )
    }
    fn status(&self) -> NetworkFuture<'_, NetworkStatus> {
        Box::pin(async { self.0.status().await.map_err(helper_error) })
    }
    fn peers(&self) -> NetworkFuture<'_, Vec<NetworkPeer>> {
        Box::pin(async { self.0.peers().await.map_err(helper_error) })
    }
    fn login(&self, key: Option<AuthKey>) -> NetworkFuture<'_, NetworkStatus> {
        Box::pin(async move {
            self.0
                .login(key.as_ref().map(AuthKey::expose))
                .await
                .map_err(helper_error)
        })
    }
    fn shutdown(&self) -> NetworkFuture<'_, ()> {
        Box::pin(async {
            // The proof is the ORIGINAL child's OS exit. A lost stop ACK is
            // permitted; ACK, EOF and cancellation alone never confirm cleanup.
            self.0
                .shutdown_confirmed()
                .await
                .map(|_exit_proof| ())
                .map_err(helper_error)
        })
    }
}
fn helper_error(_: shellfox_transport::HelperError) -> AppError {
    error(
        ErrorCode::NativeUnavailable,
        "Verified transport helper operation failed.",
    )
}
