use shellfox_transport::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

fn limits() -> RequestLimits {
    RequestLimits {
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: CancellationToken::new(),
        max_request_bytes: MAX_REQUEST_BYTES,
        max_response_bytes: MAX_RESPONSE_BYTES,
    }
}
struct Native {
    calls: Arc<AtomicUsize>,
    response: Vec<u8>,
    stall: bool,
}
impl NativeOutbound for Native {
    fn request<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(peer.as_str(), "n-peer");
            assert_eq!(endpoint, Endpoint::Request);
            assert_eq!(body, b"{}");
            assert!(limits.max_request_bytes <= MAX_REQUEST_BYTES);
            assert!(limits.max_response_bytes <= MAX_RESPONSE_BYTES);
            assert!(limits.deadline <= Instant::now() + Duration::from_secs(30));
            if self.stall {
                std::future::pending::<()>().await;
            }
            Ok(self.response.clone())
        })
    }
}
#[tokio::test]
async fn approval_and_revocation_are_unchanged_application_responses() {
    for code in ["APPROVAL_REQUIRED", "ACCESS_DENIED"] {
        let bytes = format!(
            r#"{{"ok":false,"error":{{"code":"{code}","message":"host decision","retryable":false}}}}"#
        )
        .into_bytes();
        let calls = Arc::new(AtomicUsize::new(0));
        let transport = NativeAdapter::new(Native {
            calls: calls.clone(),
            response: bytes.clone(),
            stall: false,
        });
        assert_eq!(
            transport
                .send(
                    &AuthenticatedPeerId::new("n-peer").unwrap(),
                    Endpoint::Request,
                    b"{}".to_vec(),
                    limits()
                )
                .await
                .unwrap(),
            bytes
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn validation_cancellation_deadline_and_size_limits_apply_before_native_call() {
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = NativeAdapter::new(Native {
        calls: calls.clone(),
        response: b"{}".to_vec(),
        stall: false,
    });
    let peer = AuthenticatedPeerId::new("n-peer").unwrap();
    let cancelled = limits();
    cancelled.cancel.cancel();
    assert_eq!(
        transport
            .send(&peer, Endpoint::Request, b"{}".to_vec(), cancelled)
            .await,
        Err(TransportError::Cancelled)
    );
    let mut expired = limits();
    expired.deadline = Instant::now();
    assert_eq!(
        transport
            .send(&peer, Endpoint::Request, b"{}".to_vec(), expired)
            .await,
        Err(TransportError::Deadline)
    );
    assert_eq!(
        transport
            .send(&peer, Endpoint::Request, b"not-json".to_vec(), limits())
            .await,
        Err(TransportError::InvalidRequest)
    );
    assert_eq!(
        transport
            .send(
                &peer,
                Endpoint::Request,
                vec![b' '; MAX_REQUEST_BYTES + 1],
                limits()
            )
            .await,
        Err(TransportError::TooLarge)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut expanded = limits();
    expanded.max_request_bytes = usize::MAX;
    expanded.max_response_bytes = usize::MAX;
    expanded.deadline += Duration::from_secs(60);
    assert_eq!(
        transport
            .send(&peer, Endpoint::Request, b"{}".to_vec(), expanded)
            .await
            .unwrap(),
        b"{}"
    );
}
#[tokio::test]
async fn inflight_cancellation_and_timeout_drop_native_future_without_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = NativeAdapter::new(Native {
        calls: calls.clone(),
        response: vec![],
        stall: true,
    });
    let peer = AuthenticatedPeerId::new("n-peer").unwrap();
    let request_limits = limits();
    let cancel = request_limits.cancel.clone();
    let future = transport.send(&peer, Endpoint::Request, b"{}".to_vec(), request_limits);
    let cancel_future = async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel.cancel();
    };
    let (result, _) = tokio::join!(future, cancel_future);
    assert_eq!(result, Err(TransportError::Cancelled));
    let mut short = limits();
    short.deadline = Instant::now() + Duration::from_millis(10);
    assert_eq!(
        transport
            .send(&peer, Endpoint::Request, b"{}".to_vec(), short)
            .await,
        Err(TransportError::Deadline)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn native_responses_must_be_bounded_json() {
    let peer = AuthenticatedPeerId::new("n-peer").unwrap();
    for (response, expected) in [
        (b"invalid".to_vec(), TransportError::InvalidResponse),
        (vec![b' '; MAX_RESPONSE_BYTES + 1], TransportError::TooLarge),
    ] {
        let transport = NativeAdapter::new(Native {
            calls: Arc::new(AtomicUsize::new(0)),
            response,
            stall: false,
        });
        assert_eq!(
            transport
                .send(&peer, Endpoint::Request, b"{}".to_vec(), limits())
                .await,
            Err(expected)
        );
    }
}
