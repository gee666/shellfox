use super::*;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_terminal::inspection::{InspectionPlatform, RawProcessIdentity};

fn client() -> (InspectionClient, InspectionTicket) {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let ticket = InspectionTicket {
        identity,
        sequence: 1,
    };
    let (requests, _) = mpsc::sync_channel(1);
    let shared = Arc::new(Mutex::new(State {
        identity,
        next: 1,
        pending: Some(ticket),
        result: Some((
            ticket,
            Ok(RawOwnedEvidence {
                platform: InspectionPlatform::Windows,
                root: RawProcessIdentity { pid: 1, birth: 2 },
                root_alive: false,
                complete: true,
                processes: Vec::new(),
            }),
        )),
        capability: None,
        availability: None,
        stopped: false,
    }));
    (InspectionClient { requests, shared }, ticket)
}
#[test]
fn a_stale_generation_cannot_consume_the_current_result() {
    let (client, ticket) = client();
    let stale = InspectionTicket {
        identity: Identity {
            generation: Generation::new(),
            ..ticket.identity
        },
        ..ticket
    };
    assert!(matches!(
        client.poll(stale),
        InspectionPoll::Ready(Err(InspectionFailure::OwnershipMismatch))
    ));
    assert!(
        matches!(client.poll(ticket), InspectionPoll::Ready(Ok(ProcessInspection { identity, .. })) if identity == ticket.identity)
    );
    assert!(matches!(
        client.poll(ticket),
        InspectionPoll::Ready(Err(InspectionFailure::OwnershipMismatch))
    ));
}
#[test]
fn lock_contention_is_pending_not_a_service_or_terminal_wait() {
    let (client, ticket) = client();
    let _held = client.shared.lock().unwrap();
    assert_eq!(client.request(), Err(InspectionFailure::Pending));
    assert!(matches!(client.poll(ticket), InspectionPoll::Pending));
}
#[test]
fn invalidation_drops_raw_evidence_and_fences_late_results() {
    let (client, ticket) = client();
    client.invalidate();
    let state = client.shared.lock().unwrap();
    assert!(
        state.stopped
            && state.pending.is_none()
            && state.result.is_none()
            && state.capability.is_none()
    );
    drop(state);
    assert!(matches!(
        client.poll(ticket),
        InspectionPoll::Ready(Err(InspectionFailure::Unavailable))
    ));
    assert_eq!(client.request(), Err(InspectionFailure::Unavailable));
}
