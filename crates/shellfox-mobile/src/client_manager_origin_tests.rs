//! Deterministic original-request regressions. No concurrent/aliased &mut attach claim.
use super::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
async fn issued(
    manager: &mut ManagerRecovery<Transport>,
    transport: &Transport,
) -> (RecoveryOrigin, m::Attachment) {
    let origin = manager.begin_recovery().unwrap();
    transport.ready(attachment(transport));
    let m::Reply::Success(value) = manager
        .client
        .execute_with_witness(&origin.witness, &request())
        .await
        .unwrap()
    else {
        panic!()
    };
    (origin, value)
}
fn frame(manager: &ManagerRecovery<Transport>) -> shellfox_ghostty_vt::Screen {
    crate::engine::with_engine(manager.terminal, |owned| {
        owned.terminal.render().map_err(|_| crate::Status::Engine)
    })
    .unwrap()
}
#[test]
fn network_worker_stages_but_only_original_native_owner_installs() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (mut manager, transport) = executor.block_on(manager());
    transport.ready(attachment(&transport));
    let worker = std::thread::spawn(move || {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        executor.block_on(manager.attach(request())).unwrap();
        manager
    });
    let mut manager = worker.join().unwrap();
    assert_eq!(progress(&manager), (1, 0));
    let prepared = manager.prepare_pending().unwrap();
    manager.commit_prepared(prepared).unwrap();
    assert_eq!(progress(&manager), (1, 1));
    manager.close().unwrap();
}
#[test]
fn binding_second_native_view_reuses_the_actual_client_without_pair_or_parser_replay() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (mut first, transport) = executor.block_on(manager());
    let calls = transport.calls();
    let engine = crate::engine::create(20, 5).unwrap();
    let mut second =
        ManagerRecovery::bind_native0(first.client.clone(), engine, first.receiver, first.limits)
            .unwrap();
    assert!(Arc::ptr_eq(&first.client, &second.client));
    assert_eq!(transport.calls(), calls);
    assert_eq!(progress(&second), (0, 0));
    second.close().unwrap();
    assert_eq!(
        first.client.status(),
        shellfox_client::manager_v2::Status::Connected
    );
    assert_eq!(transport.calls(), calls);
    first.close().unwrap();
}
#[test]
fn completed_attach_a_cannot_be_recertified_after_real_pair_b_same_engine() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let before = old(&manager);
        let (origin_a, value_a) = issued(&mut manager, &transport).await;
        let sender = origin_a.witness.selection().native0_sender.clone();
        manager.reconnect(id(9));
        pair(&manager, id(8)).await;
        assert_eq!(manager.client.selection().unwrap().native0_sender, sender);
        assert!(manager.stage_attachment(&origin_a, &value_a).is_err());
        unchanged(&manager, &before);
        assert_eq!(progress(&manager), (0, 0));
        let calls = transport.calls();
        assert!(manager.acknowledge_consumed().await.is_err());
        assert_eq!(transport.calls(), calls);
        // B goes through the public actual attach request, not a bare DTO setter.
        transport.ready(attachment(&transport));
        manager.attach(request()).await.unwrap();
        let candidate = manager.prepare_pending().unwrap();
        manager.commit_prepared(candidate).unwrap();
        assert_eq!(progress(&manager), (1, 1));
        assert!(manager.prepare_pending().is_err());
        manager.acknowledge_consumed().await.unwrap();
        let calls = transport.calls();
        assert!(manager.acknowledge_consumed().await.is_err());
        assert_eq!(transport.calls(), calls);
        manager.close().unwrap();
    });
}
#[test]
fn delayed_inflight_attach_a_after_actual_repair_b_never_installs() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let before = old(&manager);
        let origin = manager.begin_recovery().unwrap();
        let value = attachment(&transport);
        let (send, receive) = tokio::sync::oneshot::channel();
        transport
            .0
            .lock()
            .unwrap()
            .attachments
            .push_back(AttachmentReply::Delayed(receive));
        let input = request();
        let mut pending = Box::pin(manager.client.execute_with_witness(&origin.witness, &input));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            Pin::as_mut(&mut pending).poll(&mut cx),
            Poll::Pending
        ));
        // Shared client operations are legal &self calls; no &mut alias is used.
        manager.client.reconnect(id(9));
        pair(&manager, id(8)).await;
        send.send(value).unwrap();
        assert!(pending.await.is_err());
        unchanged(&manager, &before);
        assert_eq!(progress(&manager), (0, 0));
        let calls = transport.calls();
        assert!(manager.acknowledge_consumed().await.is_err());
        assert_eq!(transport.calls(), calls);
        transport.ready(attachment(&transport));
        manager.attach(request()).await.unwrap();
        let candidate = manager.prepare_pending().unwrap();
        manager.commit_prepared(candidate).unwrap();
        assert_eq!(progress(&manager), (1, 1));
        manager.close().unwrap();
    });
}
#[test]
fn held_origin_cannot_acquire_new_generation_or_stage_twice() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let before = old(&manager);
        let (origin_a, value_a) = issued(&mut manager, &transport).await;
        let (origin_b, value_b) = issued(&mut manager, &transport).await;
        assert_eq!(origin_a.witness.binding(), origin_b.witness.binding());
        assert!(origin_b.generation > origin_a.generation);
        assert!(manager.stage_attachment(&origin_a, &value_a).is_err());
        unchanged(&manager, &before);
        assert_eq!(progress(&manager), (0, 0));
        manager.stage_attachment(&origin_b, &value_b).unwrap();
        assert!(manager.stage_attachment(&origin_b, &value_b).is_err());
        let candidate = manager.prepare_pending().unwrap();
        manager.commit_prepared(candidate).unwrap();
        assert!(manager.stage_attachment(&origin_b, &value_b).is_err());
        assert_eq!(progress(&manager), (1, 1));
        manager.acknowledge_consumed().await.unwrap();
        manager.close().unwrap();
    });
}
#[test]
fn held_poll_data_and_revocation_cannot_be_recertified_after_same_id_pair_cycle() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (mut manager, transport) = manager().await;
        let value = attachment(&transport);
        let m::Attachment::Attached(attached) = value.clone() else {
            panic!()
        };
        transport.ready(value);
        manager.attach(request()).await.unwrap();
        let candidate = manager.prepare_pending().unwrap();
        manager.commit_prepared(candidate).unwrap();
        let origin_a = manager.installed_origin().unwrap();
        transport.event(m::Event::Data {
            delivery: attached.delivery.clone(),
            sequence: 1,
            encoding: m::Encoding::Base64,
            data: shellfox_protocol::encode_chunk(b"STALE A").unwrap(),
        });
        let m::Reply::Success(reply_a) = manager
            .client
            .poll_with_witness(&origin_a.witness)
            .await
            .unwrap()
        else {
            panic!()
        };
        manager.reconnect(id(2));
        pair(&manager, id(8)).await;
        assert_eq!(
            manager.client.binding().as_ref(),
            Some(origin_a.witness.binding())
        );
        transport.ready(attachment(&transport));
        manager.attach(request()).await.unwrap();
        let candidate = manager.prepare_pending().unwrap();
        manager.commit_prepared(candidate).unwrap();
        let before = frame(&manager);
        let tickets = progress(&manager);
        let m::HostEvent::Terminal { event } = &reply_a.events[0] else {
            panic!()
        };
        assert!(manager.observe_from(&origin_a, event).is_err());
        assert!(manager
            .observe_from(
                &origin_a,
                &m::Event::Revoked {
                    delivery: attached.delivery,
                    reason: m::RevokedReason::ControlLost
                }
            )
            .is_err());
        let after = frame(&manager);
        assert_eq!(after.cells, before.cells);
        assert_eq!(after.cursor, before.cursor);
        assert_eq!(progress(&manager), tickets);
        manager.acknowledge_consumed().await.unwrap();
        manager.close().unwrap();
    });
}
