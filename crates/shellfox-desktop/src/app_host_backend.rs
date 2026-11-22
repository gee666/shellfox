//! Injectable endpoint boundary. No view can construct a Host, PTY or controller ID.
use crate::host_client::{
    HostClient, HostEndpoint, HostEndpointConfig, HostEndpointStartupOutcome,
};
use anyhow::Result;
use shellfox_service::native::{NativeAction, NativeResponse};
use std::{future::Future, pin::Pin};

pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait AppHostBackend: Send + Sync + 'static {
    type Client: Clone + Send + Sync;
    /// The endpoint's typed bootstrap proof, never a derived readiness/error flag.
    fn startup_outcome(&self) -> HostFuture<'_, HostEndpointStartupOutcome>;
    /// Independent opaque endpoint artifact evidence, never resource-close proof.
    fn endpoint_report(
        &self,
    ) -> HostFuture<'_, Option<std::sync::Arc<shellfox_service::NativeEndpointReport>>> {
        Box::pin(async { Ok(None) })
    }
    /// Query the service's actual runtime owners. Retained/unknown is not zero.
    fn owned_terminal_count(&self) -> HostFuture<'_, usize>;
    /// Must fork a fresh client identity on the existing owner, never clone it.
    fn fork_window(&self) -> HostFuture<'_, Self::Client>;
    /// Release only this client's input intents, leases, observers and frame caches.
    fn detach_window<'a>(&'a self, client: &'a Self::Client) -> HostFuture<'a, ()>;
    /// Success means actual Stop AND endpoint/owner/helper completion, not enqueue.
    /// Preserve the source persistence report separately from resource proof.
    /// Cancellation must preserve the pending Stop so another wait can resume it.
    fn stop(&self) -> HostFuture<'_, Option<shellfox_service::ShutdownOutcome>>;
}

pub enum EndpointBackend {
    Owned(tokio::sync::Mutex<HostEndpoint>),
    Attached {
        config: HostEndpointConfig,
        runtime: crate::host_client::attached::AttachedRuntime,
        owner: shellfox_service::desktop_launch::LaunchOwnerFence,
        /// A desktop-started host: Quit confirms and stops it with its shells.
        /// Otherwise (`host run`) closing the desktop only detaches.
        owned: bool,
    },
}
/// One local request on a reactor of its own; GPUI futures have no Tokio reactor.
async fn with_local<T: Send + 'static>(
    config: &HostEndpointConfig,
    work: impl AsyncFnOnce(&shellfox_service::local::LocalClient) -> Result<T> + Send + 'static,
) -> Result<T> {
    let config = config.clone();
    let (reply, result) = tokio::sync::oneshot::channel();
    std::thread::Builder::new().name("shellfox-host-control".into()).spawn(move || {
        let outcome = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(Into::into).and_then(|runtime| {
            runtime.block_on(async {
                let client = shellfox_service::local::LocalClient::connect(&config.data_dir, &config.helper, config.helper_sha256).await?;
                work(&client).await
            })
        });
        let _ = reply.send(outcome);
    })?;
    result.await.map_err(|_| anyhow::anyhow!("Host control thread ended."))?
}
impl EndpointBackend {
    /// Move the original pre-GPU endpoint, never construct another service owner.
    pub fn from_endpoint(endpoint: HostEndpoint) -> Self {
        Self::Owned(tokio::sync::Mutex::new(endpoint))
    }
    pub fn start(config: HostEndpointConfig) -> Result<Self> {
        Self::start_with_dispatcher(config, ())
    }
    pub fn start_with_dispatcher<D: shellfox_service::OwnerDispatcher + 'static>(
        config: HostEndpointConfig,
        dispatcher: D,
    ) -> Result<Self> {
        Ok(Self::from_endpoint(HostEndpoint::start_with_dispatcher(config, dispatcher)?))
    }
}
impl AppHostBackend for EndpointBackend {
    type Client = HostClient;
    fn startup_outcome(&self) -> HostFuture<'_, HostEndpointStartupOutcome> {
        Box::pin(async { Ok(match self {
            Self::Owned(endpoint) => endpoint.lock().await.startup_outcome(),
            Self::Attached { .. } => HostEndpointStartupOutcome::Ready,
        }) })
    }
    fn endpoint_report(
        &self,
    ) -> HostFuture<'_, Option<std::sync::Arc<shellfox_service::NativeEndpointReport>>> {
        Box::pin(async { Ok(match self {
            Self::Owned(endpoint) => endpoint.lock().await.endpoint_report(),
            Self::Attached { .. } => None,
        }) })
    }
    fn owned_terminal_count(&self) -> HostFuture<'_, usize> {
        Box::pin(async {
            let endpoint = match self {
                Self::Owned(endpoint) => endpoint,
                Self::Attached { config, owned: true, .. } => {
                    return with_local(config, async |client| {
                        match client.request(&shellfox_service::local::DesktopRequest::Status).await? {
                            shellfox_service::local::DesktopReply::Status(status) => Ok(status.active_terminals),
                            _ => anyhow::bail!("Host returned an invalid status response."),
                        }
                    })
                    .await;
                }
                // A desktop attached to `host run` owns no shell.
                Self::Attached { .. } => return Ok(0),
            };
            let client = {
                let mut endpoint = endpoint.lock().await;
                match endpoint.startup_outcome() {
                    HostEndpointStartupOutcome::NoOwnerClean => return Ok(0),
                    HostEndpointStartupOutcome::RetainedCleanup => anyhow::bail!(
                        "Owned terminal count is unavailable while bootstrap cleanup is retained."
                    ),
                    HostEndpointStartupOutcome::Ready
                    | HostEndpointStartupOutcome::ReadyLocalOnly { .. } => endpoint.ready().await?,
                }
            };
            match client.execute(NativeAction::Status).await {
                Ok(NativeResponse::Status(status)) => Ok(status.active_terminals),
                Ok(_) => anyhow::bail!("Service returned an invalid native status response."),
                Err(_)
                    if self.startup_outcome().await?
                        == HostEndpointStartupOutcome::NoOwnerClean =>
                {
                    Ok(0)
                }
                Err(error) => Err(error),
            }
        })
    }
    fn fork_window(&self) -> HostFuture<'_, HostClient> {
        Box::pin(async {
            match self {
                Self::Owned(endpoint) => Ok(endpoint.lock().await.ready().await?.fork()),
                Self::Attached { config, runtime, owner, .. } => Ok(HostClient::attached(runtime.connect(config.clone(), *owner).await?))
            }
        })
    }
    fn detach_window<'a>(&'a self, client: &'a HostClient) -> HostFuture<'a, ()> {
        Box::pin(client.close_window_client())
    }
    fn stop(&self) -> HostFuture<'_, Option<shellfox_service::ShutdownOutcome>> {
        Box::pin(async {
            let endpoint = match self {
                Self::Owned(endpoint) => endpoint,
                Self::Attached { config, owned: true, .. } => {
                    crate::host_process::announce_quit();
                    with_local(config, async |client| {
                        client.execute(shellfox_service::experiment::ExperimentRequest::Stop {}).await?;
                        // Stop replies once admitted; the host exits after closing its shells.
                        client.closed().await;
                        Ok(())
                    })
                    .await?;
                    return Ok(None);
                }
                Self::Attached { .. } => return Ok(None),
            };
            let mut endpoint = endpoint.lock().await;
            endpoint.shutdown().await?;
            Ok(endpoint.shutdown_outcome())
        })
    }
}
