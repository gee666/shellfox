use super::*;
fn saved(enabled: bool) -> super::super::super::preferences::RemotePreferences {
    super::super::super::preferences::RemotePreferences {
        version: 1,
        enabled,
        hostname: "saved-host".into(),
        peers: vec![super::super::super::preferences::RememberedPeer {
            id: "n-peer".into(),
            hostname: "remembered".into(),
            addresses: vec!["100.64.0.2".into()],
        }],
    }
}
#[tokio::test]
async fn saved_enabled_preference_starts_the_node_with_the_saved_hostname() {
    let (_scratch, backend, network, directory) = fixture();
    network.online.store(true, Ordering::SeqCst);
    *backend.preferences.lock().unwrap() = Some(saved(true));
    let (control, mut runtime) = RemoteRuntime::spawn(
        backend,
        directory,
        Arc::new(Factory(network.clone())),
        "unused-fallback".into(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let state = until(&control, |state| {
        state.status == RemoteNetworkStatus::Online
    })
    .await;
    assert!(state.enabled);
    assert_eq!(state.hostname, "saved-host");
    assert!(state.attempts.is_empty());
    assert!(state.peers[0].remembered);
    assert_eq!(network.starts.load(Ordering::SeqCst), 1);
    assert_eq!(network.logins.load(Ordering::SeqCst), 0);
    runtime.shutdown().await.unwrap();
}
#[tokio::test]
async fn saved_disabled_preference_stays_off() {
    let (_scratch, backend, network, directory) = fixture();
    network.online.store(true, Ordering::SeqCst);
    *backend.preferences.lock().unwrap() = Some(saved(false));
    let (control, mut runtime) = RemoteRuntime::spawn(
        backend,
        directory,
        Arc::new(Factory(network.clone())),
        "unused-fallback".into(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let state = control.request(RemoteRequest::State).await.unwrap();
    assert!(!state.enabled);
    assert_eq!(state.hostname, "saved-host");
    assert_eq!(state.status, RemoteNetworkStatus::Disabled);
    assert!(state.attempts.is_empty());
    assert!(!state.peers[0].online && !state.peers[0].host_confirmed);
    assert_eq!(network.starts.load(Ordering::SeqCst), 0);
    runtime.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_admission_returns_only_unaccepted_actions_for_retry() {
    let (_scratch, backend, network, directory) = fixture();
    let (source, mut runtime) = RemoteRuntime::spawn(
        backend,
        directory,
        Arc::new(Factory(network.clone())),
        "fixture".into(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let (sender, receiver) = mpsc::channel(1);
    let (_state, state) = watch::channel(source.snapshot());
    let (outgoing, _outgoing_receiver) = tokio::sync::mpsc::channel(16);
    let control = RemoteControl {
        sender,
        outgoing,
        state,
        runtime: tokio::runtime::Handle::current(),
    };
    let native_control = control.clone();
    std::thread::spawn(move || {
        assert!(tokio::runtime::Handle::try_current().is_err());
        drop(native_control.outgoing_deadline(std::time::Instant::now()));
    })
    .join()
    .expect("native frontend can create an outgoing deadline outside Tokio");
    let _accepted = control.try_submit(RemoteRequest::State).unwrap();
    let rejected = control
        .try_submit(RemoteRequest::Login { auth_key: None })
        .unwrap_err();
    assert!(matches!(
        rejected,
        RemoteAdmissionError::Full(RemoteRequest::Login { .. })
    ));
    drop(receiver);
    assert!(matches!(
        control.try_submit(RemoteRequest::State),
        Err(RemoteAdmissionError::Stopped(RemoteRequest::State))
    ));
    assert_eq!(network.starts.load(Ordering::SeqCst), 0);
    runtime.shutdown().await.unwrap();
}
#[test]
fn auth_keys_and_login_urls_do_not_escape_debug_output() {
    let key = AuthKey::new("tskey-auth-FAKE-FIXTURE-ONLY".into()).unwrap();
    assert!(!format!(
        "{:?}",
        RemoteRequest::Login {
            auth_key: Some(key)
        }
    )
    .contains("FAKE-FIXTURE"));
    assert!(RemoteRequest::Cancel {
        attempt_id: Uuid::nil()
    }
    .validate()
    .is_err());
}
