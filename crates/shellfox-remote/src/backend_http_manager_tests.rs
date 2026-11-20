//! Production TCP parser/server -> existing signed Manager handler, not direct facade calls.
use super::*;
use shellfox_protocol::{manager_v2 as m, Validate};
#[derive(Clone, serde::Deserialize)]
struct Vector {
    name: String,
    path: String,
    body: String,
    peer: String,
    timestamp: String,
    signature: String,
}
fn vector(name: &str) -> Vector {
    serde_json::from_str::<Vec<Vector>>(include_str!("../tests/fixtures/manager-auth-vectors.json"))
        .unwrap()
        .into_iter()
        .find(|v| v.name == name)
        .unwrap()
}
fn backend(prepared: bool) -> Backend {
    let backend = Backend::new(
        Arc::new(OfflineOwner),
        ApprovalControl::default(),
        Secret::new("b".repeat(32)).unwrap(),
    );
    let mut backend = if prepared {
        backend.prepare_manager_v2()
    } else {
        backend
    };
    backend.clock = || 1700000001123;
    backend
}
async fn post(address: std::net::SocketAddr, request: Vector) -> (u16, Vec<u8>) {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let head=format!("POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nx-shellfox-peer: {}\r\nx-shellfox-timestamp: {}\r\nx-shellfox-signature: {}\r\n\r\n",request.path,request.body.len(),request.peer,request.timestamp,request.signature);
    let request = [head.as_bytes(), request.body.as_bytes()].concat();
    stream.write_all(&request).await.unwrap();
    let mut bytes = Vec::new();
    // A rejected route can close with unread request bytes. Darwin sometimes
    // reports a reset after the complete response rather than reporting EOF.
    if let Err(error) = stream.read_to_end(&mut bytes).await {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    let end = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    let mut headers = std::str::from_utf8(&bytes[..end]).unwrap().lines();
    let status = headers.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let length: usize = headers
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .expect("explicit response length")
        .1
        .trim()
        .parse()
        .unwrap();
    let body = &bytes[end + 4..];
    assert_eq!(body.len(), length, "the response must not be truncated");
    (status, body.to_vec())
}
#[tokio::test]
async fn production_tcp_v2_signed_discovery_and_application_error_are_not_legacy_wrapped() {
    let (_, handle) = Arc::new(backend(true)).bind().await.unwrap();
    let (status, body) = post(handle.address(), vector("discover")).await;
    assert_eq!(status, 200);
    let reply: m::Reply<m::Discovery> = serde_json::from_slice(&body).unwrap();
    reply.validate().unwrap();
    let m::Reply::Success(discovery) = reply else {
        panic!("signed production discovery failed")
    };
    assert!(discovery.profiles.iter().any(|p| p == m::PROFILE));
    let (status, body) = post(handle.address(), vector("pair")).await;
    assert_eq!(status, 200);
    let reply: m::Reply<m::PairSelection> = serde_json::from_slice(&body).unwrap();
    reply.validate().unwrap();
    assert!(
        matches!(reply, m::Reply::Failure(_)),
        "fixture owner mismatch must remain canonical application failure"
    );
    handle.shutdown().await;
}
#[tokio::test]
async fn production_tcp_v2_wrong_auth_route_and_unprepared_status_deny() {
    let (_, handle) = Arc::new(backend(true)).bind().await.unwrap();
    for change in [0, 1] {
        let mut request = vector("discover");
        if change == 0 {
            request.signature = "0".repeat(64);
        } else {
            request.peer = "e30".into();
        }
        let (status, body) = post(handle.address(), request).await;
        assert_eq!(status, 200);
        let reply: m::Reply<m::Discovery> = serde_json::from_slice(&body).unwrap();
        reply.validate().unwrap();
        assert!(matches!(reply, m::Reply::Failure(_)));
    }
    let mut request = vector("discover");
    request.path = "/v2/not-allowed".into();
    let (_, body) = post(handle.address(), request).await;
    let reply: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(reply["error"]["code"], "VALIDATION");
    handle.shutdown().await;
    let (_, handle) = Arc::new(backend(false)).bind().await.unwrap();
    let (status, body) = post(handle.address(), vector("discover")).await;
    assert_eq!(status, 404);
    assert!(body.is_empty());
    handle.shutdown().await;
}
