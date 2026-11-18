use super::*;
#[tokio::test]
async fn a_new_attempt_replaces_only_its_previous_connection_not_device_or_v1_authority() {
    let (mut backend, _, owner) = backend(Some(ViewPermission::Write));
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let old = backend
        .observer_v2
        .as_ref()
        .unwrap()
        .state
        .lock()
        .unwrap()
        .views
        .values()
        .next()
        .unwrap()
        .connection
        .clone();
    backend.observer_v2.as_mut().unwrap().mint_connection =
        || "40000000-0000-4000-8000-000000000002".parse().unwrap();
    assert_eq!(call(&backend, "pair-new-attempt").await["ok"], true);
    assert_eq!(
        call(&backend, "snapshot-repeat").await["error"]["code"],
        "AUTH_FAILED"
    );
    old.actor().with_authorized(None, || Ok(())).unwrap();
    assert!(old
        .dispatch::<()>(None, || {
            panic!("an old queued mutation must not execute")
        })
        .is_err());
    assert_eq!(call(&backend, "snapshot-new-connection").await["ok"], true);
    assert!(owner.calls.load(Ordering::SeqCst) >= 4);
}
#[tokio::test]
async fn revocation_between_queue_and_owner_commit_denies_the_actual_existing_channel() {
    let (backend, control, _) = backend(Some(ViewPermission::Write));
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let connection = backend
        .observer_v2
        .as_ref()
        .unwrap()
        .state
        .lock()
        .unwrap()
        .views
        .values()
        .next()
        .unwrap()
        .connection
        .clone();
    let (sender, mut receiver) = crate::owner_channel(1);
    let command = OwnerCommand::ObserverV2(Box::new(ObserverOwnerRequest {
        connection,
        operation: ObserverOperation::Execute(wire::Command::GetSnapshot(wire::GetSnapshot(
            shellfox_protocol::GetSnapshot::default(),
        ))),
    }));
    let task = tokio::spawn(async move { sender.request(command).await });
    let request = receiver.recv().await.unwrap();
    control.set_enabled(false).unwrap();
    struct Never;
    impl crate::ServiceHost for Never {
        fn dispatch(&mut self, _: OwnerCommand) -> crate::channel::OwnerResult {
            panic!("revoked queued operation reached Service")
        }
    }
    request.dispatch(&mut Never);
    assert_eq!(
        task.await.unwrap().unwrap_err().code,
        ErrorCode::AccessDenied
    );
}
