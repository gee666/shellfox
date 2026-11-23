use super::*;
use std::time::Instant;

async fn ready() -> (
    tempfile::TempDir,
    RemoteControl,
    Arc<FakeNetwork>,
    RemoteRuntime,
) {
    let (_scratch, backend, network, directory) = fixture();
    network.online.store(true, Ordering::SeqCst);
    let (control, runtime) = RemoteRuntime::spawn(
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
    until(&control, |state| {
        state.status == RemoteNetworkStatus::Online && !state.peers.is_empty()
    })
    .await;
    (_scratch, control, network, runtime)
}

async fn connect(control: &RemoteControl) -> Uuid {
    let receipt = control
        .try_submit(RemoteRequest::Connect {
            peer_id: "n-peer".into(),
        })
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let Some(ConnectResult::Created(id)) = receipt.connect else {
        panic!("Connect did not create an attempt");
    };
    id
}

#[tokio::test]
async fn stale_cancel_does_not_touch_the_replacement_attempt() {
    let (_scratch, control, _, mut runtime) = ready().await;
    let old = connect(&control).await;
    control
        .request(RemoteRequest::Cancel { attempt_id: old })
        .await
        .unwrap();
    let replacement = connect(&control).await;
    assert_ne!(old, replacement);
    let error = control
        .request(RemoteRequest::Cancel { attempt_id: old })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    let fresh = control.request(RemoteRequest::State).await.unwrap();
    assert_eq!(fresh.peers[0].attempt_id, Some(replacement));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancel_after_connected_returns_connected_without_disconnecting() {
    let (_scratch, control, network, mut runtime) = ready().await;
    let id = connect(&control).await;
    let pending = until(&control, |state| {
        state.peers[0].connection == RemoteConnection::AwaitingApproval
    })
    .await;
    assert_eq!(pending.peers[0].attempt_id, Some(id));
    network.allowed.store(true, Ordering::SeqCst);
    control
        .request(RemoteRequest::ContinueDesktopPair {
            attempt_id: id,
            permit: PairingPermit {
                deadline: Instant::now() + Duration::from_secs(30),
                lifetime: CancellationToken::new(),
            },
        })
        .await
        .unwrap();
    until(&control, |state| {
        state.peers[0].connection == RemoteConnection::Connected
    })
    .await;
    let reply = control
        .request(RemoteRequest::Cancel { attempt_id: id })
        .await
        .unwrap();
    assert_eq!(reply.peers[0].connection, RemoteConnection::Connected);
    assert_eq!(network.disconnects.load(Ordering::SeqCst), 0);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn established_peer_recovers_after_outage_without_replaying_requests() {
    let (_scratch, control, network, mut runtime) = ready().await;
    network.allowed.store(true, Ordering::SeqCst);
    connect(&control).await;
    until(&control, |state| state.peers[0].connection == RemoteConnection::Connected).await;
    network.peer_outage.store(true, Ordering::SeqCst);
    until(&control, |state| state.peers[0].connection == RemoteConnection::Reconnecting).await;
    assert_eq!(network.pairs.load(Ordering::SeqCst), 1);
    network.peer_outage.store(false, Ordering::SeqCst);
    let mut watch = control.subscribe();
    tokio::time::timeout(Duration::from_secs(8), async {
        while watch.borrow().peers[0].connection != RemoteConnection::Connected {
            watch.changed().await.unwrap();
        }
    }).await.unwrap();
    assert_eq!(network.pairs.load(Ordering::SeqCst), 2);
    assert_eq!(network.disconnects.load(Ordering::SeqCst), 0);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn established_peer_stops_recovering_when_approval_is_lost() {
    let (_scratch, control, network, mut runtime) = ready().await;
    network.allowed.store(true, Ordering::SeqCst);
    connect(&control).await;
    until(&control, |state| state.peers[0].connection == RemoteConnection::Connected).await;
    network.peer_outage.store(true, Ordering::SeqCst);
    until(&control, |state| state.peers[0].connection == RemoteConnection::Reconnecting).await;
    network.allowed.store(false, Ordering::SeqCst);
    network.peer_outage.store(false, Ordering::SeqCst);
    let mut watch = control.subscribe();
    tokio::time::timeout(Duration::from_secs(8), async {
        while watch.borrow().peers[0].connection != RemoteConnection::Rejected {
            watch.changed().await.unwrap();
        }
    }).await.unwrap();
    assert!(watch.borrow().peers[0].attempt_id.is_none());
    network.allowed.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(3300)).await;
    assert_eq!(network.pairs.load(Ordering::SeqCst), 2);
    assert_eq!(watch.borrow().peers[0].connection, RemoteConnection::Rejected);
    runtime.shutdown().await.unwrap();
}

#[test]
fn queued_pair_success_cannot_start_events_after_expiry_close_or_owner_shutdown() {
    let now = Instant::now();
    let scope = CancellationToken::new();
    let closed = CancellationToken::new();
    closed.cancel();
    for (deadline, lifetime, owner_stopped) in [
        (now, scope.clone(), false),
        (now + Duration::from_secs(1), closed, false),
        (now + Duration::from_secs(1), scope.clone(), true),
    ] {
        let (admit, mut events) = tokio::sync::oneshot::channel();
        let queued = peers::PairActivation::new(Some(PairingPermit { deadline, lifetime }), admit);
        assert!(queued.activate(owner_stopped, now).is_err());
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
    }
    let (admit, mut events) = tokio::sync::oneshot::channel();
    let valid = peers::PairActivation::new(
        Some(PairingPermit {
            deadline: now + Duration::from_secs(1),
            lifetime: scope,
        }),
        admit,
    );
    assert!(valid.activate(false, now).is_ok());
    events.try_recv().unwrap();
}

#[tokio::test]
async fn owner_receipt_only_claims_a_created_pair_and_rejects_expired_or_closed_ticks() {
    let (_scratch, control, network, mut runtime) = ready().await;
    let request = || RemoteRequest::Connect {
        peer_id: "n-peer".into(),
    };
    let receipt = control
        .try_submit(request())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let Some(ConnectResult::Created(id)) = receipt.connect else {
        panic!("No creation receipt");
    };
    assert_eq!(receipt.state.peers[0].attempt_id, Some(id));
    let waiting = until(&control, |state| {
        state.peers[0].connection == RemoteConnection::AwaitingApproval
    })
    .await;
    assert_eq!(waiting.peers[0].attempt_id, Some(id));
    let duplicate = control
        .try_submit(request())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(duplicate.connect, Some(ConnectResult::Existing));
    assert_eq!(duplicate.state.peers[0].attempt_id, Some(id));

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    network.allowed.store(true, Ordering::SeqCst);
    let now = Instant::now();
    for permit in [
        PairingPermit {
            deadline: now - Duration::from_secs(1),
            lifetime: CancellationToken::new(),
        },
        PairingPermit {
            deadline: now + Duration::from_secs(30),
            lifetime: cancelled,
        },
    ] {
        let error = control
            .request(RemoteRequest::ContinueDesktopPair {
                attempt_id: id,
                permit,
            })
            .await
            .unwrap_err();
        assert!(error.message.contains("Pairing stopped"));
        let state = control.snapshot();
        assert_eq!(
            state.peers[0].connection,
            RemoteConnection::AwaitingApproval
        );
        assert_eq!(state.peers[0].attempt_id, Some(id));
    }
    // The headless command remains explicit and keeps the same attempt UUID.
    control
        .request(RemoteRequest::Continue { attempt_id: id })
        .await
        .unwrap();
    until(&control, |state| {
        state.peers[0].connection == RemoteConnection::Connected
    })
    .await;
    runtime.shutdown().await.unwrap();
}
