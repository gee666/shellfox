use crate::{
    config::*,
    process::Process,
    protocol::{NetworkPeer, NetworkStatus, Probe},
    retirement::{CleanupOwner, PendingHelper},
    HelperError, PermissionVerifier, Secret,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::{watch, Mutex};
use tokio_util::sync::CancellationToken;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Success is an original-child OS exit barrier, even if its stop reply was lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelperShutdown {
    pub stop_acknowledged: bool,
}

pub struct HelperTransport {
    process: Mutex<Option<Process>>,
    pub(crate) proxy: LoopbackOrigin,
    pub(crate) proxy_secret: Secret,
    pub(crate) stopped: CancellationToken,
    statuses: watch::Receiver<Option<NetworkStatus>>,
    // Keep the isolated credential location owned, without exposing it to requests.
    state: Option<IsolatedStateDirectory>,
    cleanup: CleanupOwner,
}
impl std::fmt::Debug for HelperTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HelperTransport([redacted])")
    }
}
impl HelperTransport {
    pub async fn probe(executable: &VerifiedHelper) -> Result<Probe, HelperError> {
        let mut pending = PendingHelper::probe(executable)?;
        let process = pending.process()?;
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let raw = process.frames.recv().await.ok_or(HelperError::Stopped)??;
            let probe: Probe = serde_json::from_slice(&raw).map_err(|_| HelperError::Protocol)?;
            probe.validate()?;
            let status = process
                .child
                .wait()
                .await
                .map_err(|_| HelperError::Stopped)?;
            if !status.success() {
                return Err(HelperError::Protocol);
            }
            match process.frames.recv().await {
                Some(Err(HelperError::Stopped)) => Ok(probe),
                _ => Err(HelperError::Protocol),
            }
        })
        .await
        .map_err(|_| HelperError::Deadline)
        .and_then(|r| r);
        process.terminate().await?;
        result
    }

    /// This explicitly starts a node and may contact the coordination service.
    /// It never enrolls automatically. Tests must inject a fake executable.
    pub async fn start(config: HelperConfig) -> Result<Self, HelperError> {
        Self::start_configured(config, None).await
    }
    /// Explicit isolated-loopback control override. Default start/enrollment
    /// behavior is unchanged; this does not enroll or select a peer.
    pub async fn start_with_control_server(
        config: HelperConfig,
        control: LoopbackOrigin,
    ) -> Result<Self, HelperError> {
        Self::start_configured(config, Some(control)).await
    }
    async fn start_configured(
        config: HelperConfig,
        control: Option<LoopbackOrigin>,
    ) -> Result<Self, HelperError> {
        config.validate()?;
        if let Some(control) = &control {
            LoopbackOrigin::parse_control_url(&control.url())?;
        }
        Self::probe(&config.executable).await?;
        config
            .state_directory
            .prepare_for_start(&PermissionVerifier::new(config.executable.clone()))
            .await?;
        let mut payload = json!({
            "stateDirectory": config.state_directory.path(), "hostname": config.hostname,
            "backendUrl": config.backend.url(), "backendSecret": config.backend_secret.expose(),
            "proxySecret": config.proxy_secret.expose()
        });
        // Omit entirely by default: old verified helpers retain compatibility.
        if let Some(control) = control {
            payload["controlUrl"] = json!(control.url());
        }
        let mut pending = PendingHelper::start(&config.executable, config.state_directory)?;
        let value = pending
            .process()?
            .command("start", payload, COMMAND_TIMEOUT)
            .await?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Started {
            proxy_url: String,
        }
        let started: Started = serde_json::from_value(value).map_err(|_| HelperError::Protocol)?;
        let proxy = LoopbackOrigin::parse_proxy(&started.proxy_url)?;
        let (process, state, cleanup) = pending.finish()?;
        Ok(Self {
            statuses: process.statuses.clone(),
            stopped: process.stopped.clone(),
            process: Mutex::new(Some(process)),
            proxy,
            proxy_secret: config.proxy_secret,
            state: Some(state),
            cleanup,
        })
    }
    pub fn subscribe_status(&self) -> watch::Receiver<Option<NetworkStatus>> {
        self.statuses.clone()
    }
    pub async fn status(&self) -> Result<NetworkStatus, HelperError> {
        self.status_command("status", json!({})).await
    }
    pub async fn login(&self, auth_key: Option<&str>) -> Result<NetworkStatus, HelperError> {
        if auth_key.is_some_and(|key| !valid_auth_key(key)) {
            return Err(HelperError::Configuration);
        }
        let payload = auth_key.map_or_else(|| json!({}), |key| json!({"authKey":key}));
        self.status_command("login", payload).await
    }
    pub async fn logout(&self) -> Result<NetworkStatus, HelperError> {
        self.status_command("logout", json!({})).await
    }
    async fn status_command(
        &self,
        method: &str,
        payload: Value,
    ) -> Result<NetworkStatus, HelperError> {
        let value = self.command(method, payload).await?;
        let parsed = serde_json::from_value::<NetworkStatus>(value)
            .map_err(|_| HelperError::Protocol)
            .and_then(NetworkStatus::validate);
        if parsed.is_err() {
            self.invalidate().await?;
        }
        parsed
    }
    pub async fn peers(&self) -> Result<Vec<NetworkPeer>, HelperError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Peers {
            peers: Vec<NetworkPeer>,
        }
        let value = self.command("peers", json!({})).await?;
        let parsed = (|| {
            let peers: Peers = serde_json::from_value(value).map_err(|_| HelperError::Protocol)?;
            if peers.peers.len() > 1000 {
                return Err(HelperError::Protocol);
            }
            for peer in &peers.peers {
                peer.validate()?;
            }
            Ok(peers.peers)
        })();
        if parsed.is_err() {
            self.invalidate().await?;
        }
        parsed
    }
    async fn command(&self, method: &str, payload: Value) -> Result<Value, HelperError> {
        if self.stopped.is_cancelled() {
            return Err(HelperError::Stopped);
        }
        let mut process = self.process.try_lock().map_err(|_| HelperError::Busy)?;
        let result = process
            .as_mut()
            .ok_or(HelperError::Stopped)?
            .command(method, payload, COMMAND_TIMEOUT)
            .await;
        if result.is_err() {
            self.stopped.cancel();
        }
        result
    }
    async fn invalidate(&self) -> Result<(), HelperError> {
        self.stopped.cancel();
        // Failed termination propagates while the same controller stays owned.
        self.process
            .lock()
            .await
            .as_mut()
            .ok_or(HelperError::Stopped)?
            .terminate()
            .await
    }
    pub async fn child_exit_confirmed(&self) -> Result<bool, HelperError> {
        self.process
            .lock()
            .await
            .as_mut()
            .ok_or(HelperError::Stopped)?
            .poll_exit()
    }
    pub async fn shutdown_confirmed(&self) -> Result<HelperShutdown, HelperError> {
        self.stopped.cancel();
        let mut guard = self.process.lock().await;
        let process = guard.as_mut().ok_or(HelperError::Stopped)?;
        let stop_acknowledged = process.stop_confirmed().await?;
        Ok(HelperShutdown { stop_acknowledged })
    }
    pub async fn shutdown(&self) -> Result<(), HelperError> {
        self.shutdown_confirmed().await.map(|_| ())
    }
}
impl Drop for HelperTransport {
    fn drop(&mut self) {
        self.stopped.cancel();
        if let Some(mut process) = self.process.get_mut().take() {
            if process.poll_exit() == Ok(true) {
                drop(process);
                self.state.take();
            } else {
                self.cleanup.retain(process, self.state.take());
            }
        }
    }
}
fn valid_auth_key(key: &str) -> bool {
    key.len() <= 4096
        && key.strip_prefix("tskey-auth-").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}
