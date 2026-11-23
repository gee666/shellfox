use super::*;
#[tokio::test]
async fn shutdown_timeout_retains_manager_and_retries_until_stop_is_confirmed() {
    let (_scratch, backend, network, directory) = fixture();
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
    until(&control, |state| {
        state.status == RemoteNetworkStatus::LoginRequired
    })
    .await;
    network.allow_stop.store(false, Ordering::SeqCst);
    let error = runtime.shutdown().await.unwrap_err();
    assert!(error.message.contains("CleanupRetained"));
    assert!(control.snapshot().cleanup_retained);
    assert!(network.stops.load(Ordering::SeqCst) >= 2);
    assert_eq!(network.starts.load(Ordering::SeqCst), 1);
    assert!(matches!(
        control.try_submit(RemoteRequest::Configure {
            enabled: true,
            hostname: "fixture".into()
        }),
        Err(RemoteAdmissionError::Stopped(_))
    ));
    network.allow_stop.store(true, Ordering::SeqCst);
    runtime.shutdown().await.unwrap();
    assert!(!control.snapshot().cleanup_retained);
    assert_eq!(control.snapshot().status, RemoteNetworkStatus::Disabled);
}
