#![cfg(feature = "test-helper")]
use shellfox_transport::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
mod support;
use support::{executable, Scratch};
async fn start(mode: &str) -> (HelperTransport, Scratch) {
    let scratch = Scratch::new();
    let config = HelperConfig {
        executable: executable(),
        state_directory: IsolatedStateDirectory::create_new(scratch.0.join("node")).unwrap(),
        hostname: mode.into(),
        backend: LoopbackOrigin::new("127.0.0.1:52744".parse().unwrap()).unwrap(),
        backend_secret: Secret::new("b".repeat(32)).unwrap(),
        proxy_secret: Secret::new("p".repeat(32)).unwrap(),
    };
    (HelperTransport::start(config).await.unwrap(), scratch)
}
fn limits() -> RequestLimits {
    RequestLimits {
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: CancellationToken::new(),
        max_request_bytes: MAX_REQUEST_BYTES,
        max_response_bytes: MAX_RESPONSE_BYTES,
    }
}
#[tokio::test]
async fn actual_process_probe_commands_events_and_owned_shutdown() {
    assert_eq!(
        HelperTransport::probe(&executable())
            .await
            .unwrap()
            .protocol_version,
        HELPER_PROTOCOL_VERSION
    );
    let (helper, _scratch) = start("normal").await;
    assert_eq!(
        helper.subscribe_status().borrow().as_ref().unwrap().status,
        NetworkState::Online
    );
    assert_eq!(helper.status().await.unwrap().status, NetworkState::Online);
    assert_eq!(helper.peers().await.unwrap()[0].id, "n-peer");
    assert_eq!(
        helper.login(None).await.unwrap().status,
        NetworkState::LoginRequired
    );
    assert_eq!(helper.logout().await.unwrap().status, NetworkState::Online);
    assert_eq!(
        helper.login(Some("bad-key")).await.unwrap_err(),
        HelperError::Configuration
    );
    helper.shutdown().await.unwrap();
    assert_eq!(helper.status().await.unwrap_err(), HelperError::Stopped);
    drop(helper);
}
#[tokio::test]
async fn invalid_frames_mismatched_ids_large_lines_and_errors_fail_closed() {
    for mode in ["wrong-id", "malformed", "huge-line", "secret-error"] {
        let (helper, _scratch) = start(mode).await;
        let error = helper.status().await.unwrap_err();
        assert!(matches!(
            error,
            HelperError::Protocol | HelperError::Command
        ));
        assert!(!error.to_string().contains("tskey"));
        assert!(!format!("{helper:?}").contains("pppp"));
        assert_eq!(helper.status().await.unwrap_err(), HelperError::Stopped);
        drop(helper);
    }
}
#[tokio::test]
async fn real_loopback_proxy_preserves_denials_and_never_redirects_or_replays() {
    let (helper, _scratch) = start("normal").await;
    let peer = AuthenticatedPeerId::new("n-peer").unwrap();
    for endpoint in [
        Endpoint::Discover,
        Endpoint::Pair,
        Endpoint::Request,
        Endpoint::Events,
        Endpoint::Disconnect,
    ] {
        assert!(helper
            .send(&peer, endpoint, b"{}".to_vec(), limits())
            .await
            .is_ok());
    }
    for (case, code) in [
        ("approval", "APPROVAL_REQUIRED"),
        ("revoked", "ACCESS_DENIED"),
    ] {
        let response = helper
            .send(
                &peer,
                Endpoint::Request,
                format!(r#"{{"case":"{case}"}}"#).into_bytes(),
                limits(),
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response).unwrap()["error"]["code"],
            code
        );
    }
    assert_eq!(
        helper
            .send(
                &peer,
                Endpoint::Request,
                br#"{"case":"redirect"}"#.to_vec(),
                limits()
            )
            .await,
        Err(TransportError::Rejected { status: 302 })
    );
    assert_eq!(
        helper
            .send(
                &peer,
                Endpoint::Request,
                br#"{"case":"drop"}"#.to_vec(),
                limits()
            )
            .await,
        Err(TransportError::InvalidResponse)
    );
    assert_eq!(
        helper
            .send(
                &peer,
                Endpoint::Request,
                br#"{"case":"huge"}"#.to_vec(),
                limits()
            )
            .await,
        Err(TransportError::TooLarge)
    );
    let body = format!("\"{}\"", "x".repeat(MAX_REQUEST_BYTES - 2));
    assert_eq!(
        helper
            .send(&peer, Endpoint::Request, body.into_bytes(), limits())
            .await,
        Err(TransportError::TooLarge)
    );
    helper.shutdown().await.unwrap();
    drop(helper);
}
#[tokio::test]
async fn loopback_request_deadline_and_cancellation_do_not_stop_the_node() {
    let (helper, _scratch) = start("normal").await;
    let peer = AuthenticatedPeerId::new("n-peer").unwrap();
    let mut short = limits();
    short.deadline = Instant::now() + Duration::from_millis(10);
    assert_eq!(
        helper
            .send(
                &peer,
                Endpoint::Events,
                br#"{"case":"slow"}"#.to_vec(),
                short
            )
            .await,
        Err(TransportError::Deadline)
    );
    let request_limits = limits();
    let cancel = request_limits.cancel.clone();
    let future = helper.send(
        &peer,
        Endpoint::Events,
        br#"{"case":"slow"}"#.to_vec(),
        request_limits,
    );
    let cancelling = async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel.cancel();
    };
    let (result, ()) = tokio::join!(future, cancelling);
    assert_eq!(result, Err(TransportError::Cancelled));
    assert_eq!(helper.status().await.unwrap().status, NetworkState::Online);
    helper.shutdown().await.unwrap();
    drop(helper);
}
#[tokio::test]
async fn shutdown_cancels_inflight_poll_and_prevents_new_sends() {
    let (helper, _scratch) = start("normal").await;
    let peer = AuthenticatedPeerId::new("n-peer").unwrap();
    let poll = helper.send(
        &peer,
        Endpoint::Events,
        br#"{"case":"slow"}"#.to_vec(),
        limits(),
    );
    let shutdown = async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        helper.shutdown().await.unwrap();
    };
    let (result, ()) = tokio::join!(poll, shutdown);
    assert_eq!(result, Err(TransportError::Unavailable));
    assert_eq!(
        helper
            .send(&peer, Endpoint::Request, b"{}".to_vec(), limits())
            .await,
        Err(TransportError::Unavailable)
    );
    drop(helper);
}

#[test]
fn explicit_paths_digest_and_secret_configuration_reject_unsafe_values() {
    assert_eq!(
        VerifiedHelper::new(
            PathBuf::from(env!("CARGO_BIN_EXE_shellfox-fixture-helper")),
            [0; 32]
        )
        .unwrap_err(),
        HelperError::Executable
    );
    assert_eq!(
        VerifiedHelper::new(PathBuf::from("relative"), [0; 32]).unwrap_err(),
        HelperError::Executable
    );
    assert!(Secret::new("short".into()).is_err());
    assert!(Secret::new("\n".repeat(32)).is_err());
    assert!(LoopbackOrigin::new("100.64.0.1:80".parse().unwrap()).is_err());
    assert!(LoopbackOrigin::new("127.0.0.1:0".parse().unwrap()).is_err());
    let scratch = Scratch::new();
    assert!(IsolatedStateDirectory::create_new(scratch.0.clone()).is_err());
}
