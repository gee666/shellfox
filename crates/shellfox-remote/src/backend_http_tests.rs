use super::*;
#[path = "backend_http_manager_tests.rs"]
mod manager;
use crate::{Approval, ViewPermission};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct OfflineOwner;
impl RemoteHost for OfflineOwner {
    fn request(&self, command: OwnerCommand) -> crate::channel::OwnerFuture<'_> {
        Box::pin(async move {
            let value = match command {
                OwnerCommand::Poll { .. } => {
                    json!({"events":[{"type":"changed","event":{"revision":1,"reason":"sessions"}}]})
                }
                _ => json!({}),
            };
            Ok(value)
        })
    }
}
async fn send(address: std::net::SocketAddr, name: &str, extra_headers: &str) -> Value {
    let request = super::tests::request(name);
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let headers = format!("POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nx-shellfox-peer: {}\r\nx-shellfox-timestamp: {}\r\nx-shellfox-signature: {}\r\n{}\r\n",request.path,request.body.len(),request.peer,request.timestamp,request.signature,extra_headers);
    stream.write_all(headers.as_bytes()).await.unwrap();
    if extra_headers.is_empty() {
        stream.write_all(&request.body).await.unwrap();
    }
    // The malformed-header cases are rejected before the body. Sending one
    // anyway lets Darwin reset the socket when the server closes unread data.
    let mut bytes = vec![];
    stream.read_to_end(&mut bytes).await.unwrap();
    let body = bytes
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    serde_json::from_slice(&bytes[body..]).unwrap()
}
#[tokio::test]
async fn isolated_loopback_signed_proxy_contract_all_five_routes_and_strict_framing() {
    let approvals = ApprovalControl::default();
    approvals
        .install(Approval {
            peer_id: "n-peer".into(),
            hostname: "my laptop".into(),
            approved_at_millis: 1700000000123,
            permission: ViewPermission::Write,
            file_roots: vec![],
        })
        .unwrap();
    let mut backend = Backend::new(
        Arc::new(OfflineOwner),
        approvals,
        Secret::new("b".repeat(32)).unwrap(),
    );
    backend.clock = || 1700000000123;
    let (_, handle) = Arc::new(backend).bind().await.unwrap();
    assert_eq!(handle.address().ip(), std::net::Ipv4Addr::LOCALHOST);
    assert_eq!(
        send(handle.address(), "discover", "").await["value"]["protocolVersion"],
        1
    );
    // Duplicate authentication/content framing is rejected before any owner call.
    assert_eq!(
        send(handle.address(), "pair", "x-shellfox-peer: duplicate\r\n").await["error"]["code"],
        "VALIDATION"
    );
    assert_eq!(
        send(handle.address(), "pair", "Transfer-Encoding: chunked\r\n").await["error"]["code"],
        "VALIDATION"
    );
    assert_eq!(
        send(handle.address(), "pair", "").await["value"]["paired"],
        true
    );
    assert_eq!(send(handle.address(), "snapshot", "").await["ok"], true);
    assert_eq!(
        send(handle.address(), "events", "").await["value"]["events"][0]["type"],
        "changed"
    );
    assert_eq!(
        send(handle.address(), "disconnect", "").await["value"]["disconnected"],
        true
    );
    let address = handle.address();
    drop(handle);
    tokio::task::yield_now().await;
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
#[tokio::test]
async fn native_ownership_loss_closes_listener_and_pending_unsigned_socket() {
    let backend = Backend::new(
        Arc::new(OfflineOwner),
        ApprovalControl::default(),
        Secret::new("b".repeat(32)).unwrap(),
    );
    let lost = shellfox_protocol::CancellationToken::new();
    let (_, mut handle) = Arc::new(backend).bind_until(lost.clone()).await.unwrap();
    let address = handle.address();
    let mut pending = tokio::net::TcpStream::connect(address).await.unwrap();
    pending
        .write_all(b"POST /v1/pair HTTP/1.1\r\n")
        .await
        .unwrap();
    lost.cancel();
    tokio::time::timeout(Duration::from_secs(2), &mut handle.task)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    let mut bytes = vec![];
    let _closed = tokio::time::timeout(Duration::from_secs(2), pending.read_to_end(&mut bytes))
        .await
        .unwrap();
    assert!(bytes.is_empty());
}
