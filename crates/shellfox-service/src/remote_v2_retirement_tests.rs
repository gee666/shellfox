use super::delivery::{DeliveryFailure, ObserverDelivery};
use crate::{terminals::OwnedTerminal, Service};
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::observer_v2 as wire;
use shellfox_runtime::*;
use std::{
    sync::{mpsc, Arc, Barrier, Mutex},
    time::{Duration, Instant},
};
struct ReleaseOnDrop(Option<Arc<Barrier>>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            release.wait();
        }
    }
}
struct BlockOnce {
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Arc<Barrier>,
}
impl LiveOutputSink for BlockOnce {
    fn output(&self, _: Identity, _: u64, _: &[u8], _: Instant) -> Result<(), LiveOutputFailure> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            entered.send(()).unwrap();
            self.release.wait();
        }
        Ok(())
    }
}
#[test]
fn runtime_full_retains_retirement_while_mailbox_and_opaque_recovery_are_already_revoked() {
    let data = crate::tests::scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let (entered, receive) = mpsc::channel();
    let release = Arc::new(Barrier::new(2));
    let hook = Arc::new(BlockOnce {
        entered: Mutex::new(Some(entered)),
        release: release.clone(),
    });
    let mut runtime = Runtime::start_external_with_hooks(
        identity,
        ExternalConfig::default(),
        RuntimeHooks {
            live_output: Some(hook),
        },
    )
    .unwrap();
    let mut source = runtime.take_source().unwrap();
    service
        .terminals
        .insert(identity.tab_id, OwnedTerminal::new(runtime));
    let offer = service.observer_grant(identity.tab_id, 1).unwrap();
    let observer = offer.lease();
    let response = service
        .call(
            identity.tab_id,
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
    let response = service.call(identity.tab_id, command).unwrap();
    entry.commit_ack(&reference, &ack, &response).unwrap();
    service.remote_v2_mailboxes.insert_offer(entry).unwrap();
    service
        .adopt_observer_grant(offer, crate::grant_requests::GrantHolder::ObserverMailbox)
        .unwrap();
    source
        .try_feed(ExternalChunk {
            sequence: SourceSequence(1),
            stream: ExternalStream::Stdout,
            bytes: b"late after revoke".to_vec(),
        })
        .unwrap();
    receive.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut unblock = ReleaseOnDrop(Some(release.clone()));
    let mut full = false;
    for number in 0..=COMMAND_CAPACITY {
        match service.terminals[&identity.tab_id]
            .runtime
            .try_send(Request {
                id: RequestId(1_000_000 + number as u64),
                command: Command::ObserverAck {
                    observer,
                    sequence: 0,
                },
            }) {
            Ok(()) => {}
            Err(SubmitError::Full(_)) => {
                full = true;
                break;
            }
            Err(error) => panic!("unexpected owned fixture queue state: {error:?}"),
        }
    }
    assert!(full);
    service.disable_observer_factory_policy().unwrap();
    assert_eq!(
        service
            .remote_v2_mailboxes
            .get_mut(identity, observer)
            .unwrap()
            .poll(1024, 10)
            .unwrap_err(),
        DeliveryFailure::Stale
    );
    assert!(service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .unwrap()
        .ack_command(&reference, &ack)
        .is_err());
    assert_eq!(
        service.remote_v2_mailboxes.retirement_candidates(),
        vec![(identity, observer)],
        "Full retained exact detach custody"
    );
    unblock.0.take().unwrap().wait();
    let deadline = Instant::now() + Duration::from_secs(10);
    while service
        .remote_v2_mailboxes
        .get_mut(identity, observer)
        .is_some()
    {
        assert!(Instant::now() < deadline);
        service.pump_remote_observer_retirements().unwrap();
        service.drain(identity.tab_id).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        service.active_terminals(),
        1,
        "retiring a remote view did not stop its source"
    );
}
