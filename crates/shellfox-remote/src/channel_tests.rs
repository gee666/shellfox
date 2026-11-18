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
