//! GPUI futures run on its executor, not a Tokio reactor. Keep the verified
//! local transport on one owned reactor, distinct from the running Host.
use super::HostEndpointConfig;
use anyhow::Result;
use shellfox_protocol::{AuthenticatedPeerId, LocalRequest, RequestLimits, TransportError};
use shellfox_service::{
    desktop_launch::{
        DesktopClientToken, DesktopLaunchReply, DesktopLaunchRequest, DesktopRegistration, LaunchOwnerFence,
    },
    local::{DesktopProviderRequest, DesktopRequest, LocalClient},
    native::{NativeAction, NativeResponse},
    remote::{OutgoingV2Endpoint, OutgoingV2Response},
};
use std::sync::{Arc, Mutex};

struct RequestTask<T>(tokio::task::JoinHandle<T>);
impl<T> RequestTask<T> {
    async fn wait(mut self) -> std::result::Result<T, tokio::task::JoinError> {
        (&mut self.0).await
    }
}
impl<T> Drop for RequestTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct AttachedRuntime {
    runtime: Option<tokio::runtime::Runtime>,
    renderer: Arc<Mutex<Option<DesktopClientToken>>>,
}
impl AttachedRuntime {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            runtime: Some(tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("shellfox-attached-desktop")
                .enable_all()
                .build()?),
            renderer: Arc::default(),
        })
    }
    pub(crate) async fn connect(
        &self,
        config: HostEndpointConfig,
        owner: LaunchOwnerFence,
    ) -> Result<AttachedLocal> {
        let reactor = self
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Attached desktop reactor stopped"))?
            .handle()
            .clone();
        let local = RequestTask(reactor.spawn(async move {
            let client =
                LocalClient::connect(&config.data_dir, &config.helper, config.helper_sha256)
                    .await?;
            let DesktopLaunchReply::Route(route) =
                client.request(&DesktopLaunchRequest::Discover {}).await?
            else {
                anyhow::bail!("Attached desktop received an invalid owner receipt.");
            };
            anyhow::ensure!(
                route.owner == owner,
                "The host was replaced before the desktop attached."
            );
            Ok(client)
        }))
        .wait()
        .await??;
        Ok(AttachedLocal {
            local: Arc::new(local),
            reactor,
            owner,
            renderer: self.renderer.clone(),
        })
    }
}
impl Drop for AttachedRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

#[derive(Clone)]
pub(crate) struct AttachedLocal {
    local: Arc<LocalClient>,
    reactor: tokio::runtime::Handle,
    owner: LaunchOwnerFence,
    renderer: Arc<Mutex<Option<DesktopClientToken>>>,
}
impl AttachedLocal {
    pub(crate) async fn request<R>(&self, request: R) -> Result<R::Response>
    where
        R: LocalRequest + Send + Sync + 'static,
        R::Response: Send + 'static,
    {
        let local = self.local.clone();
        RequestTask(
            self.reactor
                .spawn(async move { local.request(&request).await }),
        )
        .wait()
        .await?
    }
    pub(crate) async fn provider_request(&self, action: NativeAction) -> Result<NativeResponse> {
        let renderer = self.renderer.lock().map_err(|_| anyhow::anyhow!("Desktop registration unavailable."))?
            .ok_or_else(|| anyhow::anyhow!("Desktop registration not ready."))?;
        let provider = DesktopProviderRequest::from_native(renderer, action)?;
        Ok(self.request(DesktopRequest::Provider(provider)).await?.into())
    }

    pub(crate) async fn register_desktop(&self) -> Result<DesktopRegistration> {
        let DesktopLaunchReply::Registered(token) = self
            .request(DesktopLaunchRequest::RegisterAttached { owner: self.owner })
            .await?
        else {
            anyhow::bail!("Unexpected attached desktop registration response.");
        };
        anyhow::ensure!(
            token.owner() == self.owner,
            "Attached desktop registration owner mismatch."
        );
        *self.renderer.lock().map_err(|_| anyhow::anyhow!("Desktop registration unavailable."))? = Some(token);
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let local = self.local.clone();
        self.reactor.spawn(async move {
            // The host holds each poll until a delivery may be ready.
            let poll = DesktopLaunchRequest::PollAttached { token };
            loop {
                let response = tokio::select! {
                    response = local.request(&poll) => response,
                    () = sender.closed() => break,
                };
                match response {
                    Ok(DesktopLaunchReply::Delivery(Some(delivery)))
                        if delivery.client == token =>
                    {
                        if sender.send(delivery).await.is_err() {
                            break;
                        }
                    }
                    Ok(DesktopLaunchReply::Delivery(None)) => {}
                    _ => break,
                }
            }
        });
        Ok(DesktopRegistration { token, receiver })
    }

    pub(crate) async fn remote(
        &self,
        request: shellfox_service::remote::native::RemoteRequest,
    ) -> Result<shellfox_service::remote::native::RemoteResponse> {
        let local = self.local.clone();
        RequestTask(
            self.reactor
                .spawn(async move { local.remote_control(request).await.map(Into::into) }),
        )
        .wait()
        .await?
    }

    pub(crate) async fn disconnect(&self) -> Result<()> {
        let local = self.local.clone();
        self.reactor
            .spawn(async move { local.disconnect().await })
            .await?
    }
    pub(crate) async fn outgoing(
        &self,
        peer: AuthenticatedPeerId,
        endpoint: OutgoingV2Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> Result<OutgoingV2Response, TransportError> {
        let local = self.local.clone();
        self.reactor
            .spawn(async move {
                local
                    .outgoing_v2_with_status(&peer, endpoint, body, limits)
                    .await
            })
            .await
            .map_err(|_| TransportError::Unavailable)?
    }
}
