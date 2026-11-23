mod retained;
mod saved;
mod pairing;
use super::*;
use shellfox_transport::{LoopbackOrigin, Secret, VerifiedHelper};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
struct FakeBackend {
    control: shellfox_remote::ApprovalControl,
    helper: VerifiedHelper,
    revokes: AtomicUsize,
    preferences: std::sync::Mutex<Option<super::super::preferences::RemotePreferences>>,
}
impl RemoteControlBackend for FakeBackend {
    fn preferences(
        &self,
    ) -> NetworkFuture<'_, Option<super::super::preferences::RemotePreferences>> {
        Box::pin(async { Ok(self.preferences.lock().unwrap().clone()) })
    }
    fn save_preferences(
        &self,
        value: super::super::preferences::RemotePreferences,
    ) -> NetworkFuture<'_, ()> {
        Box::pin(async move {
            *self.preferences.lock().unwrap() = Some(value);
            Ok(())
        })
    }
    fn approvals(&self) -> shellfox_remote::ApprovalControl {
        self.control.clone()
    }
    fn helper_config(
        &self,
        state_directory: IsolatedStateDirectory,
        hostname: String,
    ) -> Result<HelperConfig, AppError> {
        Ok(HelperConfig {
            executable: self.helper.clone(),
            state_directory,
            hostname,
            backend: LoopbackOrigin::new("127.0.0.1:9".parse().unwrap()).unwrap(),
            backend_secret: Secret::new("b".repeat(32)).unwrap(),
            proxy_secret: Secret::new("p".repeat(32)).unwrap(),
        })
    }
    fn approve_full_account<'a>(&'a self, _: &'a str) -> NetworkFuture<'a, ()> {
        Box::pin(async { Err(error(ErrorCode::Unsupported,"This network fixture has no persisted approval provider.")) })
    }
    fn revoke<'a>(&'a self, _: &'a str) -> NetworkFuture<'a, ()> {
        self.revokes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
struct FakeNetwork {
    starts: AtomicUsize,
    logins: AtomicUsize,
    stops: AtomicUsize,
    allow_stop: AtomicBool,
    allowed: AtomicBool,
    online: AtomicBool,
    peer_outage: AtomicBool,
    pairs: AtomicUsize,
    disconnects: AtomicUsize,
}
struct Factory(Arc<FakeNetwork>);
impl RemoteNetworkFactory for Factory {
    fn start(&self, _: HelperConfig) -> NetworkFuture<'_, Arc<dyn RemoteNetwork>> {
        self.0.starts.fetch_add(1, Ordering::SeqCst);
        let network = self.0.clone();
        Box::pin(async move { Ok(network as Arc<dyn RemoteNetwork>) })
    }
}
impl RemoteNetwork for FakeNetwork {
    fn status(&self) -> NetworkFuture<'_, NetworkStatus> {
        Box::pin(async {
            Ok(NetworkStatus {
                status: if self.online.load(Ordering::SeqCst) {
                    NetworkState::Online
                } else {
                    NetworkState::LoginRequired
                },
                login_url: None,
                error: None,
            })
        })
    }
    fn peers(&self) -> NetworkFuture<'_, Vec<NetworkPeer>> {
        Box::pin(async {
            Ok(vec![NetworkPeer {
                id: "n-peer".into(),
                hostname: "offline peer".into(),
                addresses: vec!["100.64.0.2".into()],
                online: !self.peer_outage.load(Ordering::SeqCst),
            }])
        })
    }
    fn login(&self, key: Option<AuthKey>) -> NetworkFuture<'_, NetworkStatus> {
        assert!(key.is_none());
        self.logins.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(NetworkStatus {
                status: NetworkState::LoginRequired,
                login_url: Some("https://login.tailscale.com/a/fake-only".into()),
                error: None,
            })
        })
    }
    fn shutdown(&self) -> NetworkFuture<'_, ()> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            if self.allow_stop.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(cleanup_retained())
            }
        })
    }
}
impl OutboundTransport for FakeNetwork {
    fn send<'a>(
        &'a self,
        _: &'a wire::AuthenticatedPeerId,
        endpoint: wire::Endpoint,
        _: Vec<u8>,
        limits: wire::RequestLimits,
    ) -> wire::TransportFuture<'a> {
        Box::pin(async move {
            if endpoint == wire::Endpoint::Pair {
                self.pairs.fetch_add(1, Ordering::SeqCst);
            }
            let response = match endpoint {
                wire::Endpoint::Discover => {
                    serde_json::json!({"ok":true,"value":{"service":"shellfox","protocolVersion":1}})
                }
                wire::Endpoint::Pair if !self.allowed.load(Ordering::SeqCst) => {
                    serde_json::json!({"ok":false,"error":{"code":"APPROVAL_REQUIRED","message":"waiting","retryable":true}})
                }
                wire::Endpoint::Pair => {
                    serde_json::json!({"ok":true,"value":{"paired":true,"terminalControl":"exclusive-v1"}})
                }
                wire::Endpoint::Disconnect => {
                    self.disconnects.fetch_add(1, Ordering::SeqCst);
                    serde_json::json!({"ok":true,"value":{"disconnected":true}})
                }
                wire::Endpoint::Events => loop {
                    if self.peer_outage.load(Ordering::SeqCst) {
                        return Err(wire::TransportError::Unavailable);
                    }
                    tokio::select! {
                        _ = limits.cancel.cancelled() => return Err(wire::TransportError::Cancelled),
                        _ = tokio::time::sleep(Duration::from_millis(25)) => {}
                    }
                },
                _ => panic!("control actor must not issue terminal mutations"),
            };
            Ok(serde_json::to_vec(&response).unwrap())
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<FakeBackend>,
    Arc<FakeNetwork>,
    IsolatedStateDirectory,
) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("remote-control-")
        .tempdir_in(root)
        .unwrap();
    let exe = scratch.path().join("not-executed-fixture");
    std::fs::write(&exe, b"fixture").unwrap();
    let hash = "f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d";
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hash[i * 2..i * 2 + 2], 16).unwrap();
    }
    let helper = VerifiedHelper::new(exe, bytes).unwrap();
    let backend = Arc::new(FakeBackend {
        control: Default::default(),
        helper,
        revokes: AtomicUsize::new(0),
        preferences: Default::default(),
    });
    let network = Arc::new(FakeNetwork {
        starts: AtomicUsize::new(0),
        logins: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        allow_stop: AtomicBool::new(true),
        allowed: AtomicBool::new(false),
        online: AtomicBool::new(false),
        peer_outage: AtomicBool::new(false),
        pairs: AtomicUsize::new(0),
        disconnects: AtomicUsize::new(0),
    });
    let directory =
        IsolatedStateDirectory::create_new(scratch.path().join("isolated-state")).unwrap();
    (scratch, backend, network, directory)
}
async fn until(
    control: &RemoteControl,
    condition: impl Fn(&RemoteControlState) -> bool,
) -> RemoteControlState {
    let mut watch = control.subscribe();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let current = watch.borrow().clone();
            if condition(&current) {
                return current;
            }
            watch.changed().await.unwrap();
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn boot_does_not_start_or_login_and_cancel_only_affects_exact_attempt() {
    let (_scratch, backend, network, directory) = fixture();
    let (control, mut runtime) = RemoteRuntime::spawn(
        backend.clone(),
        directory,
        Arc::new(Factory(network.clone())),
        "fixture".into(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        control.request(RemoteRequest::State).await.unwrap().status,
        RemoteNetworkStatus::Disabled
    );
    assert_eq!(network.starts.load(Ordering::SeqCst), 0);
    assert_eq!(network.logins.load(Ordering::SeqCst), 0);
    control
        .request(RemoteRequest::Configure {
            enabled: true,
            hostname: "fixture".into(),
        })
        .await
        .unwrap();
    until(&control, |s| s.status == RemoteNetworkStatus::LoginRequired).await;
    assert_eq!(network.starts.load(Ordering::SeqCst), 1);
    assert_eq!(network.logins.load(Ordering::SeqCst), 0);
    // A routine status request can briefly own the same admission slot.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match control.request(RemoteRequest::Login { auth_key: None }).await {
                Ok(_) => break,
                Err(error) if error.code == ErrorCode::Validation => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(error) => panic!("Unexpected login error: {error:?}"),
            }
        }
    }).await.expect("Login admission did not become available");
    let login = until(&control, |s| s.login_url.is_some()).await;
    let id = login
        .attempts
        .iter()
        .find(|a| matches!(a.kind, AttemptKind::Login))
        .unwrap()
        .id;
    assert_eq!(
        control
            .request(RemoteRequest::Cancel {
                attempt_id: Uuid::new_v4()
            })
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let canceled = control
        .request(RemoteRequest::Cancel { attempt_id: id })
        .await
        .unwrap();
    assert!(canceled.login_url.is_none());
    assert!(canceled.enabled);
    assert_eq!(backend.revokes.load(Ordering::SeqCst), 0);
    assert_eq!(network.stops.load(Ordering::SeqCst), 0);
    runtime.shutdown().await.unwrap();
    assert_eq!(network.stops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn explicit_pair_continue_and_reconnect_fence_old_attempts_without_terminal_claims() {
    let (_scratch, backend, network, directory) = fixture();
    network.online.store(true, Ordering::SeqCst);
    let (control, mut runtime) = RemoteRuntime::spawn(
        backend,
        directory,
        Arc::new(Factory(network.clone())),
        "fixture".into(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    control
        .request(RemoteRequest::Configure {
            enabled: true,
            hostname: "fixture".into(),
        })
        .await
        .unwrap();
    until(&control, |s| s.status == RemoteNetworkStatus::Online).await;
    control
        .request(RemoteRequest::Connect {
            peer_id: "n-peer".into(),
        })
        .await
        .unwrap();
    let waiting = until(&control, |s| {
        s.peers
            .iter()
            .any(|p| p.connection == RemoteConnection::AwaitingApproval)
    })
    .await;
    let old = waiting.peers[0].attempt_id.unwrap();
    network.allowed.store(true, Ordering::SeqCst);
    control
        .request(RemoteRequest::Continue { attempt_id: old })
        .await
        .unwrap();
    until(&control, |s| {
        s.peers[0].connection == RemoteConnection::Connected
    })
    .await;
    control
        .request(RemoteRequest::Reconnect {
            peer_id: "n-peer".into(),
        })
        .await
        .unwrap();
    until(&control, |s| {
        s.peers[0].connection == RemoteConnection::Connected
    })
    .await;
    assert_eq!(
        control
            .request(RemoteRequest::Cancel { attempt_id: old })
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert!(control.snapshot().incoming_legacy_terminal_attachment);
    control
        .request(RemoteRequest::Forget {
            peer_id: "n-peer".into(),
        })
        .await
        .unwrap();
    assert!(control.snapshot().peers.is_empty());
    runtime.shutdown().await.unwrap();
}
