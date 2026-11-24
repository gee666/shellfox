use super::*;
use crate::{desktop_launch::*, local::PrivateRequest};
#[tokio::test]
async fn registered_native_bootstrap_returns_queued_before_coordinator_and_reuses_one_router() {
    let data = crate::tests::scratch();
    let mut owner = ServiceOwner::start(
        data.path().into(),
        std::env::current_exe().unwrap(),
        CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    let discovery = client
        .private_request(PrivateRequest::DesktopLaunch(
            DesktopLaunchRequest::Discover {},
        ))
        .await
        .unwrap();
    let DesktopLaunchReply::Route(route) = serde_json::from_value(discovery).unwrap() else {
        panic!("missing route");
    };
    let intent = DesktopLaunchIntent::new(LaunchAction::Show {}, LaunchDisposition::ExistingWindow);
    assert!(client
        .desktop_submit(route.owner, intent.clone(), LaunchRequestKind::Fresh)
        .await
        .is_err());
    let mut registration = client.desktop_register().await.unwrap();
    let queued = client
        .desktop_submit(
            registration.token.owner(),
            intent.clone(),
            LaunchRequestKind::Fresh,
        )
        .await
        .unwrap();
    assert_eq!(queued.request_id, intent.request_id);
    assert_eq!(queued.state, LaunchState::Queued);
    let delivery = tokio::time::timeout(Duration::from_secs(2), registration.receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.intent, intent);
    assert_eq!(
        client.desktop_claim(delivery.clone()).await.unwrap(),
        LaunchClaim::Apply
    );
    assert_eq!(
        client
            .desktop_complete(delivery, LaunchHandling::Handled)
            .await
            .unwrap()
            .state,
        LaunchState::Handled
    );
    assert_eq!(
        client
            .desktop_submit(registration.token.owner(), intent, LaunchRequestKind::Fresh)
            .await
            .unwrap()
            .state,
        LaunchState::Handled
    );
    assert!(registration.receiver.try_recv().is_err());
    client.desktop_unregister(registration.token).await.unwrap();
    let shutdown = owner.shutdown().await.unwrap();
    assert_eq!(shutdown.activity, crate::ActivityShutdownState::Committed);
    assert_eq!(
        shutdown.presentation,
        crate::PresentationShutdownState::NoKnownFailure
    );
}

#[tokio::test]
async fn a_burst_larger_than_the_inbox_is_admitted() {
    let data = crate::tests::scratch();
    let mut owner = ServiceOwner::start(
        data.path().into(),
        std::env::current_exe().unwrap(),
        CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    let mut burst = tokio::task::JoinSet::new();
    for _ in 0..CAPACITY * 4 {
        let client = client.clone();
        burst.spawn(async move {
            client
                .private_request(PrivateRequest::LocalApi(LocalApiRequest::Snapshot {}))
                .await
        });
    }
    while let Some(reply) = burst.join_next().await {
        reply
            .unwrap()
            .expect("a full inbox delays a request instead of failing it");
    }
    owner.shutdown().await.unwrap();
}
