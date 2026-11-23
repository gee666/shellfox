use super::*;
#[test]
fn disabling_network_fences_old_tickets_but_does_not_revoke_device_approval() {
    let control = ApprovalControl::default();
    control.install(grant(ViewPermission::Write)).unwrap();
    let client = ClientId::new_v4();
    let actor = control.authorize(&peer(), client, 1).unwrap();
    control.set_enabled(false).unwrap();
    assert_eq!(
        actor.with_authorized(None, || Ok(())).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    assert_eq!(
        control.authorize(&peer(), client, 2).err().unwrap().code,
        ErrorCode::AccessDenied
    );
    assert_eq!(control.state().unwrap().approvals.len(), 1);
    control.set_enabled(true).unwrap();
    assert!(actor.with_authorized(None, || Ok(())).is_err());
    assert!(control
        .authorize(&peer(), client, 3)
        .unwrap()
        .with_authorized(None, || Ok(()))
        .is_ok());
}
#[tokio::test]
async fn async_policy_wait_does_not_block_tokio_supervision() {
    let control = ApprovalControl::default();
    control.install(grant(ViewPermission::Write)).unwrap();
    let actor = control.authorize(&peer(), ClientId::new_v4(), 1).unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let (done, finished) = tokio::sync::oneshot::channel();
    let owner = std::thread::spawn(move || {
        actor
            .with_authorized(None, || {
                let _ = started.send(());
                wait.recv().unwrap();
                Ok(())
            })
            .unwrap();
        let _ = done.send(());
    });
    ready.await.unwrap();
    let state = control.state_async();
    tokio::pin!(state);
    tokio::select! {_=&mut state=>panic!("policy unexpectedly unlocked"),_=tokio::time::sleep(std::time::Duration::from_millis(5))=>{}}
    release.send(()).unwrap();
    state.await.unwrap();
    finished.await.unwrap();
    owner.join().unwrap();
}
fn peer() -> PeerIdentity {
    PeerIdentity {
        id: "n-peer".into(),
        hostname: "laptop".into(),
    }
}
fn grant(permission: ViewPermission) -> Approval {
    Approval {
        peer_id: "n-peer".into(),
        hostname: "laptop".into(),
        approved_at_millis: 1,
        permission,
        file_roots: vec![],
    }
}
#[test]
fn withdrawal_tombstones_attempt_not_device_and_silent_expiry_allows_retry() {
    let control = ApprovalControl::default();
    let client = ClientId::new_v4();
    assert_eq!(
        control.authorize(&peer(), client, 1).err().unwrap().code,
        ErrorCode::ApprovalRequired
    );
    control.withdraw(&peer(), client, 2).unwrap();
    control.install(grant(ViewPermission::Write)).unwrap();
    assert_eq!(
        control.authorize(&peer(), client, 3).err().unwrap().code,
        ErrorCode::AccessDenied
    );
    assert!(control.authorize(&peer(), ClientId::new_v4(), 3).is_ok());
    assert!(control.authorize(&peer(), client, DENIED_TTL + 3).is_ok());
    let pending = ApprovalControl::default();
    assert_eq!(
        pending.authorize(&peer(), client, 1).err().unwrap().code,
        ErrorCode::ApprovalRequired
    );
    assert_eq!(
        pending
            .authorize(&peer(), client, ATTEMPT_TTL + 2)
            .err()
            .unwrap()
            .code,
        ErrorCode::ApprovalRequired
    );
}
#[test]
fn rejected_uuid_does_not_keep_a_cancelled_retry_pending() {
    let control = ApprovalControl::default();
    let peer = peer();
    let original = ClientId::new_v4();
    let retry = ClientId::new_v4();
    let now = crate::backend::now_millis();
    assert_eq!(
        control.authorize(&peer, original, now).err().unwrap().code,
        ErrorCode::ApprovalRequired
    );
    control.reject(&peer.id).unwrap();
    assert!(control.state().unwrap().pending.is_empty());
    assert_eq!(
        control.authorize(&peer, retry, now).err().unwrap().code,
        ErrorCode::ApprovalRequired
    );
    assert_eq!(control.state().unwrap().pending[0].clients, 1);
    control.withdraw(&peer, retry, now).unwrap();
    assert!(control.state().unwrap().pending.is_empty());
    for id in [original, retry] {
        assert_eq!(
            control.authorize(&peer, id, now).err().unwrap().code,
            ErrorCode::AccessDenied
        );
    }
}

#[test]
fn old_epoch_is_invalid_after_reapproval_and_attempts_are_bounded() {
    let control = ApprovalControl::default();
    control.install(grant(ViewPermission::Write)).unwrap();
    let actor = control.authorize(&peer(), ClientId::new_v4(), 1).unwrap();
    control.reject_inner("n-peer", false).unwrap();
    control.install(grant(ViewPermission::ReadOnly)).unwrap();
    assert_eq!(
        actor.with_authorized(None, || Ok(())).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    for _ in 0..255 {
        assert!(control.authorize(&peer(), ClientId::new_v4(), 2).is_ok());
    }
    assert_eq!(
        control
            .authorize(&peer(), ClientId::new_v4(), 2)
            .err()
            .unwrap()
            .code,
        ErrorCode::Validation
    );
    control.reject_inner("n-peer", true).unwrap();
    let state = control.lock().unwrap();
    assert_eq!(state.attempts["n-peer"].denied.len(), 256);
    assert!(state.pending.is_empty());
}
#[tokio::test]
async fn non_sync_owner_uses_bounded_channel_and_rechecks_queued_epoch() {
    use crate::{owner_channel, OwnerCommand, RemoteHost, ServiceHost};
    use std::{cell::Cell, rc::Rc};
    struct Owner(Rc<Cell<usize>>);
    impl ServiceHost for Owner {
        fn dispatch(&mut self, _: OwnerCommand) -> Result<serde_json::Value, AppError> {
            self.0.set(self.0.get() + 1);
            Ok(serde_json::json!({}))
        }
    }
    let control = ApprovalControl::default();
    control.install(grant(ViewPermission::Write)).unwrap();
    let actor = control.authorize(&peer(), ClientId::new_v4(), 1).unwrap();
    let (client, mut receiver) = owner_channel(1);
    let command = OwnerCommand::Pair {
        actor: actor.clone(),
    };
    let count = Rc::new(Cell::new(0));
    let mut owner = Owner(count.clone());
    let (result, ()) = tokio::join!(client.request(command), async {
        let request = receiver.recv().await.unwrap();
        control.reject_inner("n-peer", true).unwrap();
        request.dispatch(&mut owner);
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::AccessDenied);
    assert_eq!(count.get(), 0);
    let (result, ()) = tokio::join!(client.request(OwnerCommand::Disconnect { actor }), async {
        receiver.recv().await.unwrap().dispatch(&mut owner);
    });
    assert!(result.is_ok());
    assert_eq!(count.get(), 1);
}
