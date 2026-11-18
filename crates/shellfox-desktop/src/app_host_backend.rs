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

pub struct EndpointBackend {
    endpoint: tokio::sync::Mutex<HostEndpoint>,
}
impl EndpointBackend {
    /// Move the original pre-GPU endpoint, never construct another service owner.
    pub fn from_endpoint(endpoint: HostEndpoint) -> Self {
        Self {
            endpoint: tokio::sync::Mutex::new(endpoint),
        }
    }
    pub fn start(config: HostEndpointConfig) -> Result<Self> {
        Self::start_with_dispatcher(config, ())
    }
    pub fn start_with_dispatcher<D: shellfox_service::OwnerDispatcher + 'static>(
        config: HostEndpointConfig,
        dispatcher: D,
    ) -> Result<Self> {
        Ok(Self {
            endpoint: tokio::sync::Mutex::new(HostEndpoint::start_with_dispatcher(
                config, dispatcher,
            )?),
        })
    }
}
impl AppHostBackend for EndpointBackend {
    type Client = HostClient;
    fn startup_outcome(&self) -> HostFuture<'_, HostEndpointStartupOutcome> {
        Box::pin(async { Ok(self.endpoint.lock().await.startup_outcome()) })
    }
    fn endpoint_report(
        &self,
    ) -> HostFuture<'_, Option<std::sync::Arc<shellfox_service::NativeEndpointReport>>> {
        Box::pin(async { Ok(self.endpoint.lock().await.endpoint_report()) })
    }
    fn owned_terminal_count(&self) -> HostFuture<'_, usize> {
        Box::pin(async {
            let client = {
                let mut endpoint = self.endpoint.lock().await;
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
        Box::pin(async { Ok(self.endpoint.lock().await.ready().await?.fork()) })
    }
    fn detach_window<'a>(&'a self, client: &'a HostClient) -> HostFuture<'a, ()> {
        Box::pin(client.detach_all())
    }
    fn stop(&self) -> HostFuture<'_, Option<shellfox_service::ShutdownOutcome>> {
        Box::pin(async {
            let mut endpoint = self.endpoint.lock().await;
            endpoint.shutdown().await?;
            Ok(endpoint.shutdown_outcome())
        })
    }
}
