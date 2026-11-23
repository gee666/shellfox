use crate::{
    activity_integration::read_source,
    local_api::{LocalApiRequest, LocalApiResponse},
};

#[tokio::test(flavor = "current_thread")]
async fn facts_getter_returns_the_same_actual_owner_channel_and_invalidates_on_stop() {
    let data = crate::tests::scratch();
    let mut owner = crate::owner::ServiceOwner::start(
        data.path().to_owned(),
        std::env::current_exe().unwrap(),
        shellfox_protocol::CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    owner.activate().await.unwrap();
    let client = owner.client(uuid::Uuid::new_v4());
    let watch = client.source_facts_watch().await.unwrap();
    let second = client.source_facts_watch().await.unwrap();
    assert!(watch.same_channel(&second));
    let facts = read_source(&watch).expect("owner published actual source");
    let LocalApiResponse::Snapshot(snapshot) = client
        .execute(LocalApiRequest::Snapshot {})
        .await
        .unwrap()
    else {
        panic!("snapshot");
    };
    assert_eq!(facts.snapshot, snapshot);
    assert!(facts.terminals.is_empty());
    owner.shutdown().await.unwrap();
    assert!(read_source(&watch).is_none());
}

#[test]
fn source_channel_is_pending_before_any_owner_capture() {
    let activity = crate::activity::service::ServiceActivity::new().unwrap();
    assert!(read_source(&activity.watch).is_none());
}
