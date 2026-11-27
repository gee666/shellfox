use super::*;
use std::time::Instant;
#[tokio::test]
async fn peer_requests_beyond_the_in_flight_limit_wait_instead_of_failing() {
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
    until(&control, |s| s.peers.iter().any(|p| p.id == "n-peer" && p.online)).await;
    let peer = wire::AuthenticatedPeerId::new("n-peer").unwrap();
    let limit = super::super::outgoing::MAX_OUTGOING;
    let requests: Vec<_> = (0..limit + 4)
        .map(|_| {
            let control = control.clone();
            let peer = peer.clone();
            tokio::spawn(async move {
                let limits = wire::RequestLimits {
                    deadline: Instant::now() + Duration::from_secs(10),
                    cancel: CancellationToken::new(),
                    max_request_bytes: wire::MAX_REQUEST_BYTES,
                    max_response_bytes: wire::MAX_RESPONSE_BYTES,
                };
                control
                    .peer_request(&peer, wire::Endpoint::Request, b"{}".to_vec(), limits)
                    .await
            })
        })
        .collect();
    let calls = |n| {
        let network = network.clone();
        async move {
            tokio::time::timeout(Duration::from_secs(3), async {
                while network.outgoing_calls.load(Ordering::SeqCst) < n {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        }
    };
    calls(limit).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(network.outgoing_calls.load(Ordering::SeqCst), limit);
    assert!(requests.iter().all(|request| !request.is_finished()));
    // Each finished request admits one queued request.
    network.outgoing_gate.add_permits(1);
    calls(limit + 1).await;
    network.outgoing_gate.add_permits(limit + 3);
    for request in requests {
        assert_eq!(request.await.unwrap().unwrap().status, 200);
    }
    runtime.shutdown().await.unwrap();
}
