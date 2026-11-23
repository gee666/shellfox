//! The local API listener state shared by the accept loop and its connections.
use super::*;

pub(super) struct ServerState {
    pub stop: shellfox_protocol::CancellationToken,
    /// Set only by a Stop request or a stop signal, never by a failure. Clients
    /// are told an orderly stop apart from a host that died.
    pub orderly: shellfox_protocol::CancellationToken,
    pub tasks: tokio::task::JoinSet<Result<connection::ConnectionOutcome>>,
    pub lifetimes: tokio::task::JoinSet<()>,
    pub lifetime_slots: std::sync::Arc<tokio::sync::Semaphore>,
    pub lifetime_cancel: shellfox_protocol::CancellationToken,
}
impl ServerState {
    pub fn new() -> Self {
        Self {
            stop: shellfox_protocol::CancellationToken::new(),
            orderly: shellfox_protocol::CancellationToken::new(),
            tasks: tokio::task::JoinSet::new(),
            lifetimes: tokio::task::JoinSet::new(),
            lifetime_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(128)),
            lifetime_cancel: shellfox_protocol::CancellationToken::new(),
        }
    }
    pub fn request_stop(&self) {
        self.stop.cancel();
    }
    pub fn ensure_running(&self) -> Result<()> {
        anyhow::ensure!(!self.stop.is_cancelled(), "Local host stop requested");
        Ok(())
    }
    fn watch(&mut self, lifetime: connection::LifetimeConnection) {
        self.lifetimes
            .spawn(lifetime.watch(self.lifetime_cancel.clone(), self.orderly.clone()));
    }
    pub fn adopt(&mut self, outcome: Result<connection::ConnectionOutcome>) {
        if let Ok(connection::ConnectionOutcome::Lifetime(lifetime)) = outcome {
            self.watch(lifetime);
        }
    }
    pub async fn drain(&mut self, client: Option<crate::OwnerClient>, alive: bool) {
        self.request_stop();
        // Keep all accepted response-consumption and opening-epoch bookkeeping,
        // including after unwind of the listener loop. Each connection has its deadline.
        while let Some(result) = self.tasks.join_next().await {
            if let Ok(outcome) = result {
                self.adopt(outcome);
            }
        }
        self.lifetime_cancel.cancel();
        while self.lifetimes.join_next().await.is_some() {}
        if alive {
            if let Some(client) = client {
                // Best effort only. An exited actor cannot receive this command;
                // the following original actor ACK/join remains mandatory.
                let _ = client.close_all_lifetimes().await;
            }
        }
    }
}

/// Ctrl+C, or the stop signal a service manager, `kill`, or a Windows logoff
/// or shutdown sends to a host.
pub(super) async fn stop_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let (mut terminate, mut hangup) = (
            signal(SignalKind::terminate())?,
            signal(SignalKind::hangup())?,
        );
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
            _ = hangup.recv() => Ok(()),
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        let (mut close, mut logoff, mut shutdown, mut brk) = (
            windows::ctrl_close()?,
            windows::ctrl_logoff()?,
            windows::ctrl_shutdown()?,
            windows::ctrl_break()?,
        );
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = close.recv() => Ok(()),
            _ = logoff.recv() => Ok(()),
            _ = shutdown.recv() => Ok(()),
            _ = brk.recv() => Ok(()),
        }
    }
    #[cfg(not(any(unix, windows)))]
    tokio::signal::ctrl_c().await
}
