use super::*;

#[tokio::test(flavor = "current_thread")]
async fn cancelled_ack_wait_resumes_ack_without_reapplying_native_window_effect() {
    let (client, mut coordinator) = DeliveryClient::new();
    let mut windows = Windows::default();
    client.queue(request());
    client.wait_ack.set(true);
    {
        let mut handling = std::pin::pin!(coordinator.pump(&client, &mut windows));
        poll_pending(handling.as_mut());
    }
    assert_eq!(windows.calls.len(), 1);
    assert_eq!(client.claims.get(), 1);
    client.wait_ack.set(false);
    assert_eq!(
        coordinator
            .pump(&client, &mut windows)
            .await
            .unwrap()
            .unwrap()
            .state,
        LaunchState::Handled
    );
    assert_eq!(windows.calls.len(), 1);
    assert_eq!(client.claims.get(), 1);
    assert_eq!(client.acknowledgements.get(), 2);
    assert_eq!(client.broker.borrow().host.creations, 1);
    assert_eq!(coordinator.pump(&client, &mut windows).await.unwrap(), None);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_native_apply_is_uncertain_and_never_opens_a_second_window() {
    let (client, mut coordinator) = DeliveryClient::new();
    let mut windows = Windows {
        wait: true,
        ..Default::default()
    };
    let request = request();
    client.queue(request.clone());
    {
        let mut handling = std::pin::pin!(coordinator.pump(&client, &mut windows));
        poll_pending(handling.as_mut());
    }
    assert_eq!(windows.calls.len(), 1);
    windows.wait = false;
    assert_eq!(
        coordinator
            .pump(&client, &mut windows)
            .await
            .unwrap()
            .unwrap()
            .state,
        LaunchState::Uncertain
    );
    assert_eq!(windows.calls.len(), 1);
    assert_eq!(client.broker.borrow().host.creations, 1);
    client.queue(request);
    assert_eq!(coordinator.pump(&client, &mut windows).await.unwrap(), None);
    assert_eq!(windows.calls.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn stale_ownership_prevents_native_focus_without_killing_or_replacing_any_window() {
    let (client, mut coordinator) = DeliveryClient::new();
    let mut windows = Windows::default();
    client.queue(request());
    client.broker.borrow_mut().host.lost = true;
    assert_eq!(
        coordinator.pump(&client, &mut windows).await,
        Err(LaunchError::StaleOwner)
    );
    assert!(windows.calls.is_empty());
    assert_eq!(
        coordinator.pump(&client, &mut windows).await,
        Err(LaunchError::StaleClient)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn deleted_known_target_rejects_claim_then_allows_a_later_explicit_launch() {
    let (client, mut coordinator) = DeliveryClient::new();
    let mut windows = Windows::default();
    client.queue(request());
    client.broker.borrow_mut().host.sessions.clear();
    assert_eq!(
        coordinator.pump(&client, &mut windows).await,
        Err(LaunchError::ReceiptUnavailable)
    );
    assert!(windows.calls.is_empty());
    client.queue(request());
    assert_eq!(
        coordinator
            .pump(&client, &mut windows)
            .await
            .unwrap()
            .unwrap()
            .state,
        LaunchState::Handled
    );
    assert_eq!(windows.calls.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_ack_reply_does_not_repeat_already_applied_focus_or_session_creation() {
    let (client, mut coordinator) = DeliveryClient::new();
    let mut windows = Windows::default();
    client.queue(request());
    client.invalid_ack.set(true);
    assert_eq!(
        coordinator.pump(&client, &mut windows).await,
        Err(LaunchError::Invalid)
    );
    assert_eq!(windows.calls.len(), 1);
    client.invalid_ack.set(false);
    assert_eq!(
        coordinator
            .pump(&client, &mut windows)
            .await
            .unwrap()
            .unwrap()
            .state,
        LaunchState::Handled
    );
    assert_eq!(windows.calls.len(), 1);
    assert_eq!(client.claims.get(), 1);
    assert_eq!(client.broker.borrow().host.creations, 1);
}
