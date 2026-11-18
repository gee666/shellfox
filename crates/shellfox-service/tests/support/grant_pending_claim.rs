use super::*;
#[test]
fn fresh_controller_id_cannot_bypass_unknown_claim_for_exact_source() {
    let identity = identity();
    let mut ledger = GrantCustody::default();
    let (ticket, _guard) = claim(&mut ledger, identity, 1, 101);
    assert!(matches!(ledger.poll(ticket).unwrap(), GrantPoll::Pending));
    assert!(matches!(
        ledger.reserve(
            identity,
            RequestId(2),
            GrantKind::Controller(202),
            CancellationToken::new()
        ),
        Err(CustodyError::Conflict)
    ));
    // Another real generation is independent; no tab-only/global fence.
    let replacement = Identity {
        generation: Generation::new(),
        ..identity
    };
    let (other, mut other_guard) = ledger
        .reserve(
            replacement,
            RequestId(3),
            GrantKind::Controller(303),
            CancellationToken::new(),
        )
        .unwrap();
    ledger.not_sent(other).unwrap();
    other_guard.disarm();
    // Once the ACTUAL first grant is known, another authorized Claim may enter.
    // This does not invent/adopt a second lease or turn pending into Busy.
    granted(
        &mut ledger,
        identity,
        1,
        Lease {
            controller: 101,
            epoch: 8,
        },
    );
    let (next, mut next_guard) = ledger
        .reserve(
            identity,
            RequestId(4),
            GrantKind::Controller(202),
            CancellationToken::new(),
        )
        .unwrap();
    ledger.not_sent(next).unwrap();
    next_guard.disarm();
    assert!(matches!(
        ledger.poll(ticket).unwrap(),
        GrantPoll::Granted(_)
    ));
}
