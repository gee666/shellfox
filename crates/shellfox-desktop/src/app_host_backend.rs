//! Injectable host boundary. No view can construct a Host, PTY or controller ID.
use crate::host_client::{HostClient, HostEndpointConfig};
use anyhow::Result;
use std::{future::Future, pin::Pin};

pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait AppHostBackend: Send + Sync + 'static {
    type Client: Clone + Send + Sync;
    /// The host's actual live terminals. Unknown is an error, never zero.
    fn owned_terminal_count(&self) -> HostFuture<'_, usize>;
    /// Must fork a fresh client identity on the existing owner, never clone it.
    fn fork_window(&self) -> HostFuture<'_, Self::Client>;
    /// Release only this client's input intents, leases, observers and frame caches.
    fn detach_window<'a>(&'a self, client: &'a Self::Client) -> HostFuture<'a, ()>;
    /// Success means the host closed its shells and ended, not that Stop was sent.
    /// Cancellation may repeat the wait; Stop itself is idempotent.
    fn stop(&self) -> HostFuture<'_, ()>;
}

/// The host process this desktop attaches to over the local API.
pub struct AttachedBackend {
    pub(super) config: HostEndpointConfig,
    pub(super) runtime: crate::host_client::attached::AttachedRuntime,
    pub(super) route: crate::host_process::Route,
    /// A desktop-started host: Quit confirms and stops it with its shells.
    /// Otherwise (`host run`) closing the desktop only detaches.
    pub(super) owned: std::sync::atomic::AtomicBool,
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
impl AttachedBackend {
    pub(super) fn owned(&self) -> bool {
        self.owned.load(std::sync::atomic::Ordering::SeqCst)
    }
}
impl AppHostBackend for AttachedBackend {
    type Client = HostClient;
    fn owned_terminal_count(&self) -> HostFuture<'_, usize> {
        Box::pin(async {
            // A desktop attached to `host run` owns no shell.
            if !self.owned() {
                return Ok(0);
            }
            self.route.get().await?;
            with_local(&self.config, async |client| {
                match client.request(&shellfox_service::local::DesktopRequest::Status).await? {
                    shellfox_service::local::DesktopReply::Status(status) => Ok(status.active_terminals),
                    _ => anyhow::bail!("Host returned an invalid status response."),
                }
            })
            .await
        })
    }
    fn fork_window(&self) -> HostFuture<'_, HostClient> {
        Box::pin(async {
            let owner = self.route.get().await?.info.owner;
            Ok(HostClient::attached(self.runtime.connect(self.config.clone(), owner).await?))
        })
    }
    fn detach_window<'a>(&'a self, client: &'a HostClient) -> HostFuture<'a, ()> {
        Box::pin(client.close_window_client())
    }
    fn stop(&self) -> HostFuture<'_, ()> {
        Box::pin(async {
            if !self.owned() || self.route.get().await.is_err() {
                return Ok(());
            }
            crate::host_process::announce_quit();
            with_local(&self.config, async |client| {
                client.execute(shellfox_service::experiment::ExperimentRequest::Stop {}).await?;
                // Stop replies once admitted; the host exits after closing its shells.
                client.closed().await;
                Ok(())
            })
            .await
            .or_else(|error| {
                // A host that no longer accepts clients has already stopped or died.
                if crate::host_process::serving(&self.config) { Err(error) } else { Ok(()) }
            })
        })
    }
}
