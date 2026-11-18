use super::delivery::ObserverDelivery;
use crate::{terminals::OwnedTerminal, Service};
use shellfox_core::{CreateSession, Generation, ProfileId, SessionId};
use shellfox_protocol::observer_v2 as wire;
use shellfox_runtime::*;
use std::time::{Duration, Instant};

#[test]
fn existing_service_drain_routes_real_observer_bytes_once_and_keeps_them_after_lost_poll() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let snapshot = service
        .host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("View".into()),
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("automation-pipe").unwrap(),
        })
        .unwrap();
    let session = &snapshot.sessions[0];
    let tab = session.tabs[0].id;
    let identity = Identity {
        session_id: session.id,
        tab_id: tab,
        generation: Generation::new(),
    };
    let mut runtime = Runtime::start_external(identity, ExternalConfig::default()).unwrap();
    let mut source = runtime.take_source().unwrap();
    service.terminals.insert(tab, OwnedTerminal::new(runtime));
    service.activity.next_poll_ms = 0;
    service.pump_activity();
    let captured = crate::activity_integration::read_source(&service.activity.watch).unwrap();
    let descriptor = captured
        .terminals
        .iter()
        .find(|entry| entry.identity == identity)
        .unwrap();
    assert_eq!(descriptor.source_kind, Some(SourceKind::ExternalBytes));
    assert!(matches!(
        super::source_kind(Some(descriptor)),
        wire::Fact::Known {
            value: wire::SourceKind::ExternalBytes
        }
    ));
    let viewer = service.allocate_viewer_id().unwrap();
    let offer = service.observer_grant(tab, viewer).unwrap();
    let observer = offer.lease();
    let response = service
        .call(
            tab,
            Command::ObserverRecoverStamped {
                observer,
                capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
            },
        )
        .unwrap();
    let mut entry = ObserverDelivery::new(identity, observer).unwrap();
    let recovery = entry.recover(response).unwrap();
    let reference = entry.reference().clone();
    let ack = wire::Ack::Recovery {
        recovery_id: recovery.recovery_id,
        sequence: recovery.last_sequence,
    };
    let command = entry.ack_command(&reference, &ack).unwrap();
    service.remote_v2_mailboxes.insert_offer(entry).unwrap();
    service
        .adopt_observer_grant(offer, crate::grant_requests::GrantHolder::ObserverMailbox)
        .unwrap();
    let response = service.call(tab, command).unwrap();
    service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .unwrap()
        .commit_ack(&reference, &ack, &response)
        .unwrap();
    source
        .try_feed(ExternalChunk {
            sequence: SourceSequence(1),
            stream: ExternalStream::Stdout,
            bytes: b"one authoritative stream".to_vec(),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        service.drain(tab).unwrap();
        let pending = service
            .remote_v2_mailboxes
            .get_mut(identity, observer)
            .unwrap()
            .poll(1024, 10)
            .unwrap();
        if !pending.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let pending = service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .unwrap()
        .poll(1024, 10)
        .unwrap();
    let [wire::Event::Data { sequence, data, .. }] = &pending[..] else {
        panic!("one event")
    };
    assert_eq!(
        wire::decode_bytes(data, 16384).unwrap(),
        b"one authoritative stream"
    );
    let ack = wire::Ack::Data {
        sequence: *sequence,
    };
    let command = service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .unwrap()
        .ack_command(&reference, &ack)
        .unwrap();
    let response = service.call(tab, command).unwrap();
    service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .unwrap()
        .commit_ack(&reference, &ack, &response)
        .unwrap();
    assert!(service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .unwrap()
        .poll(1024, 10)
        .unwrap()
        .is_empty());
    service.retire_remote_observer(identity, observer).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .is_some()
    {
        assert!(
            Instant::now() < deadline,
            "real Unobserve reply did not confirm retirement"
        );
        service.pump_remote_observer_retirements().unwrap();
        service.drain(tab).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        !service.terminals[&tab]
            .runtime
            .external_status()
            .unwrap()
            .closed
    );
    assert!(service.host.session_creation_facts().unwrap().is_empty());
}
