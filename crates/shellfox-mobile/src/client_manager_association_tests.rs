//! Public safe Rust association tests with local transport fixtures, not grants.
use super::*;
fn setup() -> (
    Arc<ManagerClient<Transport>>,
    Arc<ManagerClient<Transport>>,
    Transport,
    VerifiedNativeExtension,
) {
    let receiver = VerifiedNativeExtension::verify().unwrap();
    let identity = String::from_utf8(receiver.identity().as_bytes().to_vec()).unwrap();
    let transport = Transport(
        Arc::new(Mutex::new(Replies::default())),
        m::Native0Codec::new(m::EngineIdentity::try_from(identity).unwrap()),
    );
    let a = ManagerRecovery::native_client(
        AuthenticatedPeerId::new("fixture-peer").unwrap(),
        id(2),
        transport.clone(),
    )
    .unwrap();
    let b = ManagerRecovery::native_client(
        AuthenticatedPeerId::new("fixture-peer").unwrap(),
        id(9),
        transport.clone(),
    )
    .unwrap();
    (a, b, transport, receiver)
}
#[test]
fn duplicate_engine_same_and_different_clients_reject_without_mutating_first() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (a, b, transport, receiver) = setup();
        let mut first =
            ManagerRecovery::from_shared_native0(a.clone(), 20, 5, SnapshotLimits::default())
                .unwrap();
        pair(&first, id(3)).await;
        let before = old(&first);
        let tickets = progress(&first);
        let calls = transport.calls();
        for client in [a.clone(), b] {
            assert!(matches!(
                ManagerRecovery::bind_native0(
                    client,
                    first.terminal_handle(),
                    receiver,
                    SnapshotLimits::default()
                ),
                Err(ManagerRecoveryError::Engine(crate::Status::Busy))
            ));
            assert_eq!(progress(&first), tickets);
            unchanged(&first, &before);
            assert_eq!(transport.calls(), calls);
        }
        let attached = attachment(&transport);
        let m::Attachment::Attached(ref value) = attached else {
            panic!()
        };
        let delivery = value.delivery.clone();
        transport.ready(attached);
        first.attach(request()).await.unwrap();
        first.owner_view().pump().unwrap();
        first.acknowledge_consumed().await.unwrap();
        let suffix = b"255;0;0mFIRST";
        transport.event(m::Event::Data {
            delivery,
            sequence: 1,
            encoding: m::Encoding::Base64,
            data: shellfox_protocol::encode_chunk(suffix).unwrap(),
        });
        let poll = ManagerPollContext::capture(a.as_ref(), &[&first]).unwrap();
        poll.poll(a.as_ref()).await.unwrap();
        first.owner_view().pump().unwrap();
        first.acknowledge_consumed().await.unwrap();
        assert!(first
            .owner_view()
            .frame()
            .unwrap()
            .cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>()
            .contains("FIRST"));
        first.close().unwrap();
    });
}
#[test]
fn drop_with_retained_owner_and_poll_keeps_claim_until_original_engine_closes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (a, b, transport, receiver) = setup();
        let mut first =
            ManagerRecovery::from_shared_native0(a.clone(), 20, 5, SnapshotLimits::default())
                .unwrap();
        pair(&first, id(3)).await;
        let value = attachment(&transport);
        let m::Attachment::Attached(ref attached) = value else {
            panic!()
        };
        let delivery = attached.delivery.clone();
        transport.ready(value);
        first.attach(request()).await.unwrap();
        let owner = first.owner_view();
        owner.pump().unwrap();
        first.acknowledge_consumed().await.unwrap();
        let frame = owner.frame().unwrap();
        let engine = first.terminal_handle();
        let poll = ManagerPollContext::capture(a.as_ref(), &[&first]).unwrap();
        drop(first);
        for client in [a.clone(), b.clone()] {
            assert!(matches!(
                ManagerRecovery::bind_native0(client, engine, receiver, SnapshotLimits::default()),
                Err(ManagerRecoveryError::Engine(crate::Status::Busy))
            ));
        }
        transport.event(m::Event::Data {
            delivery,
            sequence: 1,
            encoding: m::Encoding::Base64,
            data: shellfox_protocol::encode_chunk(b"255;0;0mLATE").unwrap(),
        });
        let m::Reply::Success(stale) = poll.poll(a.as_ref()).await.unwrap() else {
            panic!("fixture poll")
        };
        assert!(stale.events.is_empty()); // One retired target does not fail other views' global poll.
        owner.pump().unwrap();
        assert_eq!(owner.frame().unwrap().cells, frame.cells);
        owner.close().unwrap();
        assert!(ManagerRecovery::bind_native0(
            b.clone(),
            engine,
            receiver,
            SnapshotLimits::default()
        )
        .is_err());
        let mut fresh =
            ManagerRecovery::from_shared_native0(b, 20, 5, SnapshotLimits::default()).unwrap();
        assert_ne!(fresh.terminal_handle(), engine);
        fresh.close().unwrap();
    });
}
