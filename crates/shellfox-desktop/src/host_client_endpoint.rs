use super::{HostClient, NativeClient};
use anyhow::{Context, Result, ensure};
use shellfox_service::OwnerDispatcher;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use tokio::sync::{mpsc, oneshot, watch};
#[path = "host_client_startup.rs"]
mod startup;
pub use startup::HostEndpointStartupOutcome;

type Ready = Arc<Mutex<Option<oneshot::Sender<Result<NativeClient>>>>>;
#[cfg(test)]
#[path = "host_client_endpoint_tests.rs"]
mod tests;
#[derive(Clone)]
pub struct HostEndpointConfig {
    pub data_dir: PathBuf,
    pub helper: PathBuf,
    pub helper_sha256: [u8; 32],
}
/// Keep this at application scope, not per terminal/window. Dropping a view or
/// this handle does not request host shutdown. Explicit shutdown is confirmed
/// through the original local closing/drain state, with owners retained on error.
pub struct HostEndpoint {
    ready: Option<oneshot::Receiver<Result<NativeClient>>>,
    done: Option<oneshot::Receiver<Result<()>>>,
    shutdown: mpsc::Sender<oneshot::Sender<Result<()>>>,
    thread: Option<JoinHandle<()>>,
    client: Option<HostClient>,
    shutdown_pending: Option<oneshot::Receiver<Result<()>>>,
    stop_acknowledged: bool,
    startup: watch::Receiver<HostEndpointStartupOutcome>,
    shutdown_report: watch::Receiver<Option<shellfox_service::ShutdownOutcome>>,
    endpoint_report: watch::Receiver<Option<Arc<shellfox_service::NativeEndpointReport>>>,
    startup_failure: watch::Receiver<Option<Arc<anyhow::Error>>>,
}
impl HostEndpoint {
    /// Starts a background async endpoint; does not wait for helper/PTY/metadata.
    pub fn start(config: HostEndpointConfig) -> Result<Self> {
        Self::start_with_dispatcher(config, ())
    }
    pub fn start_with_dispatcher<D: OwnerDispatcher + 'static>(
        config: HostEndpointConfig,
        dispatcher: D,
    ) -> Result<Self> {
        Self::start_plan(config, dispatcher, None)
    }
    pub fn start_remote(
        config: HostEndpointConfig,
        remote: shellfox_service::local::RemoteBootOptions,
    ) -> Result<Self> {
        Self::start_plan(config, (), Some(remote))
    }
    fn start_plan<D: OwnerDispatcher + 'static>(
        config: HostEndpointConfig,
        dispatcher: D,
        remote: Option<shellfox_service::local::RemoteBootOptions>,
    ) -> Result<Self> {
        ensure!(
            config.data_dir.is_absolute() && config.helper.is_absolute(),
            "Explicit absolute host data/helper paths required."
        );
        let (ready_tx, ready) = oneshot::channel();
        let ready_tx: Ready = Arc::new(Mutex::new(Some(ready_tx)));
        let (done_tx, done) = oneshot::channel();
        let (startup_tx, startup) = watch::channel(HostEndpointStartupOutcome::RetainedCleanup);
        let (shutdown_tx, shutdown_report) = watch::channel(None);
        let (endpoint_tx, endpoint_report) = watch::channel(None);
        let (failure_tx, startup_failure) = watch::channel(None);
        let (shutdown, mut stopping) = mpsc::channel::<oneshot::Sender<Result<()>>>(1);
        let stop_unclaimed_ready = shutdown.clone();
        let thread = std::thread::Builder::new().name("shellfox-desktop-endpoint".into()).spawn(move || {
            let result = (|| -> Result<()> {
                let executor = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(executor) => executor,
                    Err(error) => {
                        startup_tx.send_replace(HostEndpointStartupOutcome::NoOwnerClean);
                        return Err(error.into());
                    }
                };
                let mut bootstrap = match shellfox_service::local::LocalBootstrap::plan(
                    &config.data_dir, &config.helper, config.helper_sha256,
                ) {
                    Ok(bootstrap) => bootstrap,
                    Err(error) => {
                        // Planning has not entered any helper or native create.
                        startup_tx.send_replace(HostEndpointStartupOutcome::NoOwnerClean);
                        return Err(error);
                    }
                };
                let stop = bootstrap.stop_handle();
                let status_callback = |report: shellfox_service::local::LocalBootReport| {
                    use shellfox_service::local::LocalBootStatus;
                    if let Some(shutdown) = report.shutdown { shutdown_tx.send_replace(Some(shutdown)); }
                    if let Some(endpoint) = report.endpoint { endpoint_tx.send_replace(Some(endpoint)); }
                    startup_tx.send_replace(match report.status {
                        LocalBootStatus::NoOwnerClean => HostEndpointStartupOutcome::NoOwnerClean,
                        LocalBootStatus::RetainedCleanup => HostEndpointStartupOutcome::RetainedCleanup,
                        LocalBootStatus::Ready => HostEndpointStartupOutcome::Ready,
                        LocalBootStatus::ReadyLocalOnly { remote_unavailable } => HostEndpointStartupOutcome::ReadyLocalOnly { remote_unavailable },
                    });
                };
                let mut pending_reply = None;
                // Original bootstrap, runtime and pending Stop receipt stay OUTSIDE
                // the polled/unwinding run future on this same existing thread.
                let operation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| executor.block_on(async {
                    let callback = ready_tx.clone();
                    let ready_callback = move |owner| {
                        if let Ok(mut ready) = callback.lock()
                            && let Some(ready) = ready.take()
                            && ready.send(Ok(NativeClient::new(owner))).is_err() {
                            let (reply, _) = oneshot::channel();
                            let _ = stop_unclaimed_ready.try_send(reply);
                        }
                    };
                    let host = async {
                        if let Some(remote) = remote {
                            bootstrap.run_remote(remote, ready_callback, &status_callback).await
                        } else {
                            bootstrap.run_with_dispatcher(dispatcher, ready_callback, &status_callback).await
                        }
                    };
                    let control = async {
                        if let Some(reply) = stopping.recv().await {
                            pending_reply = Some(reply);
                            stop.request();
                        }
                        // Channel closure is detach, never implicit global Stop.
                        std::future::pending::<()>().await;
                    };
                    tokio::pin!(host, control);
                    tokio::select! { biased; _ = &mut control => unreachable!("control remains live"), result = &mut host => result }
                })));
                let result = operation.unwrap_or_else(|_| Err(bootstrap.retain_error(anyhow::anyhow!("Local host execution panicked; original cleanup retained."))));
                while *startup_tx.borrow() != HostEndpointStartupOutcome::NoOwnerClean {
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        executor.block_on(bootstrap.cleanup(&status_callback));
                    })).is_err() {
                        // A cleanup panic/error cannot detach the only keeper.
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                if let Some(reply) = pending_reply.take() { let _ = reply.send(Ok(())); }
                result
            })();
            let failure = result.err().map(Arc::new);
            failure_tx.send_replace(failure.clone());
            if let Ok(mut ready) = ready_tx.lock()
                && let Some(ready) = ready.take() {
                let message = failure.as_ref().map(ToString::to_string).unwrap_or_else(|| "Host ended before becoming ready.".into());
                let _ = ready.send(Err(anyhow::anyhow!(message)));
            }
            let result = failure.map_or(Ok(()), |error| Err(anyhow::anyhow!(error.to_string())));
            let _ = done_tx.send(result);
        })?;
        Ok(Self {
            ready: Some(ready),
            done: Some(done),
            shutdown,
            thread: Some(thread),
            client: None,
            shutdown_pending: None,
            stop_acknowledged: false,
            startup,
            shutdown_report,
            endpoint_report,
            startup_failure,
        })
    }
    pub fn startup_outcome(&self) -> HostEndpointStartupOutcome {
        *self.startup.borrow()
    }
    pub fn shutdown_outcome(&self) -> Option<shellfox_service::ShutdownOutcome> {
        self.shutdown_report.borrow().clone()
    }
    pub fn endpoint_report(&self) -> Option<Arc<shellfox_service::NativeEndpointReport>> {
        self.endpoint_report.borrow().clone()
    }
    pub fn startup_failure(&self) -> Option<Arc<anyhow::Error>> {
        self.startup_failure.borrow().clone()
    }
    pub fn occupied_owner(&self) -> Option<shellfox_service::local::OccupiedOwnerError> {
        if self.startup_outcome() != HostEndpointStartupOutcome::NoOwnerClean {
            return None;
        }
        self.startup_failure
            .borrow()
            .as_ref()
            .and_then(|error| error.downcast_ref::<shellfox_service::local::OccupiedOwnerError>())
            .cloned()
    }
    fn finish_clean(&mut self) {
        self.thread.take();
        self.ready = None;
        self.done = None;
        self.client = None;
        self.shutdown_pending = None;
    }
    /// Cancellation leaves the pending receiver here, so startup can be awaited
    /// again. The GUI never synchronously waits on the service owner thread.
    pub async fn ready(&mut self) -> Result<HostClient> {
        if let Some(client) = &self.client {
            return Ok(client.clone());
        }
        let native = self
            .ready
            .as_mut()
            .context("Host startup already failed.")?
            .await;
        self.ready = None;
        let native = native.context("Host endpoint stopped during startup.")??;
        let client = HostClient::new(native);
        self.client = Some(client.clone());
        Ok(client)
    }
    /// Only call after the application's explicit confirmed shutdown policy.
    /// A failed Stop keeps this handle usable for retry; no numeric PID cleanup.
    pub async fn shutdown(&mut self) -> Result<()> {
        match self.startup_outcome() {
            HostEndpointStartupOutcome::NoOwnerClean => {
                self.finish_clean();
                return Ok(());
            }
            HostEndpointStartupOutcome::RetainedCleanup
            | HostEndpointStartupOutcome::Ready
            | HostEndpointStartupOutcome::ReadyLocalOnly { .. } => {}
        }
        ensure!(
            self.thread.is_some(),
            "Endpoint controller is unavailable; cleanup is unconfirmed."
        );
        if !self.stop_acknowledged {
            if self.shutdown_pending.is_none() {
                let (reply, result) = oneshot::channel();
                self.shutdown.try_send(reply).map_err(|_| {
                    anyhow::anyhow!("Host shutdown queue is full or stopped; ownership retained.")
                })?;
                self.shutdown_pending = Some(result);
            }
            // Keep the receiver in self across cancellation. A second UI wait
            // resumes the same Stop; it never blindly resubmits a mutation.
            let result = self
                .shutdown_pending
                .as_mut()
                .context("Missing Stop receiver.")?
                .await;
            self.shutdown_pending = None;
            if self.startup_outcome() == HostEndpointStartupOutcome::NoOwnerClean {
                self.finish_clean();
                return Ok(());
            }
            result.context("Host shutdown reply lost; cleanup is unconfirmed.")??;
            self.stop_acknowledged = true;
        }
        let completion = self
            .done
            .as_mut()
            .context("Host completion was already consumed.")?
            .await;
        self.done = None;
        if self.startup_outcome() == HostEndpointStartupOutcome::NoOwnerClean {
            self.finish_clean();
            return Ok(());
        }
        completion.context("Host cleanup confirmation lost.")??;
        anyhow::bail!(
            "Service ended without an explicit no-owner cleanup proof; controller retained."
        )
    }
}
