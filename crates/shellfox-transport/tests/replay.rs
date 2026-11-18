#![cfg(feature = "test-helper")]
use shellfox_transport::*;
use std::time::{Duration, Instant};
mod support;
use support::{executable, Scratch};
#[tokio::test]
async fn uncertain_post_and_redirect_each_dispatch_only_once() {
    let scratch = Scratch::new();
    let helper = HelperTransport::start(HelperConfig {
        executable: executable(),
        state_directory: IsolatedStateDirectory::create_new(scratch.0.join("node")).unwrap(),
        hostname: "normal".into(),
        backend: LoopbackOrigin::new("127.0.0.1:52744".parse().unwrap()).unwrap(),
        backend_secret: Secret::new("b".repeat(32)).unwrap(),
        proxy_secret: Secret::new("p".repeat(32)).unwrap(),
    })
    .await
    .unwrap();
    let peer = AuthenticatedPeerId::new("n-peer").unwrap();
    let limits = || RequestLimits {
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: CancellationToken::new(),
        max_request_bytes: MAX_REQUEST_BYTES,
        max_response_bytes: MAX_RESPONSE_BYTES,
    };
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
                br#"{"case":"redirect"}"#.to_vec(),
                limits()
            )
            .await,
        Err(TransportError::Rejected { status: 302 })
    );
    let bytes = helper
        .send(
            &peer,
            Endpoint::Request,
            br#"{"case":"count"}"#.to_vec(),
            limits(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["calls"],
        3
    );
    helper.shutdown().await.unwrap();
    drop(helper);
}
