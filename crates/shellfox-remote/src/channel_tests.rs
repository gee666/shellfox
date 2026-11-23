use super::*;
use crate::{Approval, ApprovalControl, ViewPermission};
fn command() -> OwnerCommand {
    let control = ApprovalControl::default();
    control
        .install(Approval {
            peer_id: "n-owner-cancel".into(),
            hostname: "offline".into(),
            approved_at_millis: 1,
            permission: ViewPermission::Write,
            file_roots: Vec::new(),
        })
        .unwrap();
    OwnerCommand::Pair {
        actor: control
            .authorize(
                &shellfox_transport::incoming::PeerIdentity {
                    id: "n-owner-cancel".into(),
                    hostname: "offline".into(),
                },
                shellfox_protocol::ClientId::new_v4(),
                1,
            )
            .unwrap(),
    }
}
#[tokio::test]
async fn dropping_actual_post_send_request_future_cancels_its_owner_token() {
    let (client, mut owner) = owner_channel(1);
    let command = command();
    let wait = tokio::spawn(async move { client.request(command).await });
    let request = owner.recv().await.unwrap();
    let token = request.cancellation_token();
    assert!(!token.is_cancelled());
    wait.abort();
    assert!(wait.await.unwrap_err().is_cancelled());
    assert!(token.is_cancelled());
    assert!(request.is_cancelled());
    request.complete(Ok(serde_json::json!({"paired":true})));
    assert!(token.is_cancelled());
}
#[tokio::test]
async fn caller_timeout_drops_the_same_request_future_token_without_detached_polling() {
    let (client, mut owner) = owner_channel(1);
    let command = command();
    let wait = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_millis(5), client.request(command)).await
    });
    let request = owner.recv().await.unwrap();
    let token = request.cancellation_token();
    assert!(wait.await.unwrap().is_err());
    assert!(token.is_cancelled());
    assert!(request.is_cancelled());
}
#[tokio::test]
async fn lost_owner_reply_cancels_canonical_recipient_lifetime() {
    let (client, mut owner) = owner_channel(1);
    let command = command();
    let wait = tokio::spawn(async move { client.request(command).await });
    let request = owner.recv().await.unwrap();
    let token = request.cancellation_token();
    drop(request);
    let error = wait.await.unwrap().unwrap_err();
    assert!(!error.retryable);
    assert!(token.is_cancelled());
}
#[tokio::test]
async fn completed_reply_keeps_exact_outcome_and_ends_request_lifetime() {
    let (client, mut owner) = owner_channel(1);
    let command = command();
    let wait = tokio::spawn(async move { client.request(command).await });
    let request = owner.recv().await.unwrap();
    let token = request.cancellation_token();
    assert!(!token.is_cancelled());
    let result = serde_json::json!({"paired":true});
    request.complete(Ok(result.clone()));
    assert_eq!(wait.await.unwrap().unwrap(), result);
    assert!(token.is_cancelled());
}
#[tokio::test]
async fn wake_runs_once_per_queued_request_and_not_on_refusal() {
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = wakes.clone();
    let (client, mut owner) = owner_channel_with_wake(
        1,
        Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
    );
    let queued = client.clone();
    let wait = tokio::spawn(async move { queued.request(command()).await });
    let request = owner.recv().await.unwrap();
    assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 1);
    let blocker = client.clone();
    let blocked = tokio::spawn(async move { blocker.request(command()).await });
    while wakes.load(std::sync::atomic::Ordering::SeqCst) < 2 {
        tokio::task::yield_now().await;
    }
    // The channel is full now; a refused request must not wake the owner.
    let refused = client.request(command()).await.unwrap_err();
    assert_eq!(refused.code, ErrorCode::NativeUnavailable);
    assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 2);
    request.complete(Ok(serde_json::json!({})));
    wait.await.unwrap().unwrap();
    owner
        .recv()
        .await
        .unwrap()
        .complete(Ok(serde_json::json!({})));
    blocked.await.unwrap().unwrap();
}
