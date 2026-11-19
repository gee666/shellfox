//! Async helper/control owner. The Service and GPUI never cross this boundary.
#[cfg(test)]
#[path = "remote_runtime_tests.rs"]
mod tests;
use super::{native::*, preferences, RemoteBackend};
use shellfox_protocol::{self as wire, AppError, CancellationToken, ErrorCode, OutboundTransport};
use shellfox_transport::{
    HelperConfig, HelperTransport, IsolatedStateDirectory, NetworkPeer, NetworkState, NetworkStatus,
};
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;
#[path = "remote_runtime_actions.rs"]
mod actions;
#[path = "remote_runtime_cleanup.rs"]
mod cleanup;
#[path = "remote_runtime_discovery.rs"]
mod discovery;
#[path = "remote_runtime_events.rs"]
mod events;
#[path = "remote_runtime_lifecycle.rs"]
mod lifecycle;
#[path = "remote_network.rs"]
mod network;
#[path = "remote_network_health.rs"]
mod health;
#[path = "remote_runtime_outgoing.rs"]
mod outgoing;
#[path = "remote_runtime_peers.rs"]
mod peers;
pub use network::{
    GoNetworkFactory, NetworkFuture, RemoteControlBackend, RemoteNetwork, RemoteNetworkFactory,
};
fn cleanup_retained() -> AppError {
    error(ErrorCode::NativeUnavailable,"CleanupRetained: owned transport helper stop is not confirmed; endpoint and ownership must remain held.")
}
pub struct RemoteRuntime {
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
    failed: bool,
}
impl RemoteRuntime {
    /// Boot is disabled and never calls start/login. Only explicit Configure(true)
    /// may start the injected network factory. Proof names this isolated node.
    pub async fn spawn(
        backend: Arc<dyn RemoteControlBackend>,
        directory: IsolatedStateDirectory,
        factory: Arc<dyn RemoteNetworkFactory>,
        hostname: String,
        lost: CancellationToken,
    ) -> Result<(RemoteControl, Self), AppError> {
        RemoteRequest::Configure {
            enabled: false,
            hostname: hostname.clone(),
        }
        .validate()?;
        let policy = backend.approvals();
        policy.set_enabled_async(false).await?;
        let approval = policy.state_async().await?;
        let preferences =
            backend
                .preferences()
                .await?
                .unwrap_or(super::preferences::RemotePreferences {
                    version: 1,
                    enabled: false,
                    hostname,
                    peers: vec![],
                });
        preferences.validate()?;
        let hostname = preferences.hostname.clone();
        let mut initial=RemoteControlState{enabled:false,hostname,status:RemoteNetworkStatus::Disabled,login_url:None,error:None,peers:preferences.peers.iter().map(|peer|RemotePeer{id:peer.id.clone(),hostname:peer.hostname.clone(),addresses:peer.addresses.clone(),online:false,remembered:true,host_confirmed:false,connection:RemoteConnection::Disconnected,last_error:None,attempt_id:None}).collect(),pending:approval.pending,approvals:approval.approvals,attempts:vec![],cleanup_retained:false,incoming_legacy_terminal_attachment:true,terminal_capability_reason:"Approved devices can attach to ordinary host-account terminals.".into()};
        if preferences.enabled {
            initial.attempts.push(RemoteAttempt {
                id: Uuid::new_v4(),
                kind: AttemptKind::Resume,
            });
        }
        let (sender, receiver) = mpsc::channel(16);
        let (outgoing, outgoing_receiver) = mpsc::channel(16);
        let (state, watch) = watch::channel(initial.clone());
        let (events, incoming) = mpsc::channel(64);
        let stop = CancellationToken::new();
        let cancel = stop.clone();
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            error(ErrorCode::NativeUnavailable, "Remote control requires the owned async runtime.")
        })?;
        let task = runtime.spawn(async move {
                let mut actor = Manager {
                    backend,
                    directory,
                    factory,
                    state: initial,
                    publish: state,
                    events,
                    network: None,
                    epoch: 0,
                    cycle: CancellationToken::new(),
                    jobs: tokio::task::JoinSet::new(),
                    owned_jobs: tokio::task::JoinSet::new(),
                    orphan: None,
                    owned_failed: false,
                    connections: HashMap::new(),
                    health_busy: false,
                    preferences,
                    discovery: HashMap::new(),
                    probe_busy: false,
                    outgoing_fences: Vec::new(),
                };
                actor
                    .run(receiver, outgoing_receiver, incoming, cancel, lost)
                    .await;
            });
        Ok((
            RemoteControl {
                sender,
                outgoing,
                state: watch,
                runtime,
            },
            Self {
                stop,
                task: Some(task),
                failed: false,
            },
        ))
    }
    /// A timeout is NOT cleanup success. This object and manager retain the
    /// helper, retry confirmed stop, and must stay owned until a later Ok.
    pub async fn shutdown(&mut self) -> Result<(), AppError> {
        self.stop.cancel();
        if self.failed {
            return Err(cleanup_retained());
        }
        let Some(task) = self.task.as_mut() else {
            return Ok(());
        };
        match tokio::time::timeout(Duration::from_secs(5), task).await {
            Ok(Ok(())) => {
                self.task.take();
                Ok(())
            }
            Ok(Err(_)) => {
                self.failed = true;
                Err(cleanup_retained())
            }
            Err(_) => Err(cleanup_retained()),
        }
    }
}
impl Drop for RemoteRuntime {
    fn drop(&mut self) {
        // Detach the retrying cleanup owner, never abort it and call that clean.
        self.stop.cancel();
    }
}
struct Connection {
    id: Uuid,
    cancel: CancellationToken,
}
enum Event {
    Started {
        epoch: u64,
        id: Uuid,
        previous_confirmed: bool,
        result: Result<Arc<dyn RemoteNetwork>, AppError>,
    },
    Health {
        epoch: u64,
        id: Option<Uuid>,
        result: Result<(NetworkStatus, Vec<NetworkPeer>), AppError>,
    },
    Paired {
        epoch: u64,
        id: Uuid,
        peer: String,
        result: Result<(), AppError>,
    },
    PeerFailed {
        epoch: u64,
        id: Uuid,
        peer: String,
        error: AppError,
    },
    Stopped {
        orphan: bool,
        network: Arc<dyn RemoteNetwork>,
        result: Result<(), AppError>,
    },
    Finished,
    Discovered {
        epoch: u64,
        peers: Vec<(String, bool)>,
    },
}
struct Manager {
    backend: Arc<dyn RemoteControlBackend>,
    directory: IsolatedStateDirectory,
    factory: Arc<dyn RemoteNetworkFactory>,
    state: RemoteControlState,
    publish: watch::Sender<RemoteControlState>,
    events: mpsc::Sender<Event>,
    network: Option<Arc<dyn RemoteNetwork>>,
    epoch: u64,
    cycle: CancellationToken,
    jobs: tokio::task::JoinSet<()>,
    owned_jobs: tokio::task::JoinSet<Event>,
    orphan: Option<Arc<dyn RemoteNetwork>>,
    owned_failed: bool,
    connections: HashMap<String, Connection>,
    health_busy: bool,
    preferences: super::preferences::RemotePreferences,
    discovery: HashMap<String, std::time::Instant>,
    probe_busy: bool,
    outgoing_fences: Vec<std::sync::Weak<super::outgoing::OutgoingFence>>,
}
impl Manager {
    async fn run(
        &mut self,
        mut requests: mpsc::Receiver<RemoteEnvelope>,
        mut outgoing: mpsc::Receiver<super::outgoing::OutgoingEnvelope>,
        mut events: mpsc::Receiver<Event>,
        stop: CancellationToken,
        lost: CancellationToken,
    ) {
        let mut approvals = self.backend.approvals().subscribe();
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        self.emit().await;
        loop {
            while self.jobs.try_join_next().is_some() {}
            tokio::select! {
                _=stop.cancelled()=>break,
                _=lost.cancelled()=>break,
                Some(envelope)=outgoing.recv()=>self.outgoing_request(envelope),
                Some(envelope)=requests.recv()=>{
                    // Enqueue accepted mutations into this owner, not its reply
                    // receiver. Response loss cannot discard an accepted action.
                    if !envelope.reply.is_closed() || !matches!(&envelope.request, RemoteRequest::State){
                        let result=self.request(envelope.request).await;
                        self.emit().await;
                        let _=envelope.reply.send(result.map(|_|self.state.clone()));
                    }
                },
                Some(event)=events.recv()=>{self.apply(event).await;self.emit().await;},
                _=approvals.changed()=>self.emit().await,
                _=tick.tick()=>{if !self.state.enabled || self.state.cleanup_retained{self.retry_retained();}else if self.owned_jobs.is_empty() && self.network.is_some() && !self.health_busy{self.health(None,None);}},
                Some(event)=self.owned_jobs.join_next(),if !self.owned_jobs.is_empty()=>{match event {Ok(event)=>self.lifecycle_event(event).await,Err(_)=>{self.owned_failed=true;self.state.cleanup_retained=true;self.state.error=Some(cleanup_retained());}}self.emit().await;},
                Some(_)=self.jobs.join_next(),if !self.jobs.is_empty()=>{},
                else=>break,
            }
        }
        outgoing.close();
        while let Ok(envelope) = outgoing.try_recv() {
            let _ = envelope.reply.send(Err(wire::TransportError::Unavailable));
        }
        requests.close();
        while let Ok(envelope) = requests.try_recv() {
            let _ = envelope.reply.send(Err(cleanup_retained()));
        }
        self.cleanup_confirmed().await;
    }
    async fn emit(&mut self) {
        self.prune_outgoing();
        if let Ok(approval) = self.backend.approvals().state_async().await {
            self.state.pending = approval.pending;
            self.state.approvals = approval.approvals;
        }
        self.publish.send_replace(self.state.clone());
    }
    fn spawn_job(&mut self, future: impl Future<Output = Event> + Send + 'static) {
        let events = self.events.clone();
        let cancel = self.cycle.clone();
        self.jobs.spawn(async move {tokio::select! {_=cancel.cancelled()=>{},event=future=>{let _=events.send(event).await;}}});
    }
}
