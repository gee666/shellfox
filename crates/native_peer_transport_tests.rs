use super::*;
use shellfox_protocol::CancellationToken;
use shellfox_service::remote::OutgoingV2Response;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct Owner {
    status: u16,
    body: Vec<u8>,
    seen: Arc<Mutex<Vec<OutgoingV2Endpoint>>>,
}
impl ServiceOwner for Owner {
    fn outgoing<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: OutgoingV2Endpoint,
        _: Vec<u8>,
        limits: RequestLimits,
    ) -> OutgoingV2Future<'a> {
        assert_eq!(peer.as_str(), "n-fixture");
        assert_eq!(limits.max_response_bytes, 1024);
        self.seen.lock().unwrap().push(endpoint);
        Box::pin(std::future::ready(Ok(OutgoingV2Response {
            status: self.status,
            body: self.body.clone(),
        })))
    }
}
fn limits() -> RequestLimits {
    RequestLimits {
        deadline: Instant::now() + Duration::from_secs(1),
        cancel: CancellationToken::new(),
        max_request_bytes: 1024,
        max_response_bytes: 1024,
    }
}
#[tokio::test]
async fn business_denial_body_is_not_http_404_or_legacy_inference() {
    let peer = AuthenticatedPeerId::new("n-fixture").unwrap();
    let body =
        br#"{"error":{"code":"APPROVAL_REQUIRED","message":"404 is diagnostic text, not HTTP"}}"#
            .to_vec();
    let seen = Arc::default();
    let transport = ServiceTransport::new(Owner {
        status: 200,
        body: body.clone(),
        seen,
    });
    assert_eq!(
        transport
            .send(&peer, Endpoint::PairV2, b"{}".to_vec(), limits())
            .await
            .unwrap(),
        body
    );
    let transport = ServiceTransport::new(Owner {
        status: 404,
        body: br#"{"code":"ACCESS_DENIED"}"#.to_vec(),
        seen: Arc::default(),
    });
    assert_eq!(
        transport
            .send(&peer, Endpoint::PairV2, b"{}".to_vec(), limits())
            .await,
        Err(TransportError::Rejected { status: 404 })
    );
}
#[tokio::test]
async fn exactly_five_v2_operations_reach_retained_owner_and_v1_never_does() {
    let peer = AuthenticatedPeerId::new("n-fixture").unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport = ServiceTransport::new(Owner {
        status: 200,
        body: b"{}".to_vec(),
        seen: seen.clone(),
    });
    for endpoint in [
        Endpoint::DiscoverV2,
        Endpoint::PairV2,
        Endpoint::RequestV2,
        Endpoint::EventsV2,
        Endpoint::DisconnectV2,
    ] {
        transport
            .send(&peer, endpoint, b"{}".to_vec(), limits())
            .await
            .unwrap();
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [
            OutgoingV2Endpoint::Discover,
            OutgoingV2Endpoint::Pair,
            OutgoingV2Endpoint::Request,
            OutgoingV2Endpoint::Events,
            OutgoingV2Endpoint::Disconnect
        ]
    );
    for endpoint in [
        Endpoint::Discover,
        Endpoint::Pair,
        Endpoint::Request,
        Endpoint::Events,
        Endpoint::Disconnect,
    ] {
        assert_eq!(
            transport
                .send(&peer, endpoint, b"{}".to_vec(), limits())
                .await,
            Err(TransportError::InvalidRequest)
        );
    }
    assert_eq!(seen.lock().unwrap().len(), 5);
}
