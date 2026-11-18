use super::*;
#[test]
fn actual_reply_errors_create_no_claim_or_observer_grant_and_do_not_consume_epochs_or_prior_recovery(
) {
    let mut m = Manager::start("--child-input-boundary", "RUNTIME_INPUT_BOUNDARY_READY");
    let lease = m.lease();
    let ready = m.recover();
    m.ack(&ready).unwrap();
    m.events.clear();
    assert_eq!(
        m.request(Command::Claim {
            controller: 999,
            takeover: false
        })
        .unwrap_err()
        .code,
        ErrorCode::ControlRequired
    );
    assert!(
        !m.events
            .iter()
            .any(|event| matches!(event, Event::ControllerRevoked { .. })),
        "denied actual Claim must not revoke existing real grant"
    );
    m.request(Command::CheckControllerReady { lease }).unwrap();
    assert!(matches!(
        m.request(Command::InteractiveBytes {
            lease,
            bytes: b"a".to_vec()
        })
        .unwrap(),
        Response::Input(InputReceipt {
            accepted_bytes: 1,
            ..
        })
    ));
    let observer = match m
        .request(Command::Observe {
            viewer: 50,
            after: None,
        })
        .unwrap()
    {
        Response::Observed(observer) => observer,
        other => panic!("{other:?}"),
    };
    let snapshot = match m
        .request(Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        })
        .unwrap()
    {
        Response::Recovery(snapshot) => snapshot,
        other => panic!("{other:?}"),
    };
    for viewer in [50, 99] {
        assert_eq!(
            m.request(Command::Observe {
                viewer,
                after: Some(u64::MAX)
            })
            .unwrap_err()
            .code,
            ErrorCode::Validation
        );
    }
    m.request(Command::ObserverLayoutAck {
        observer,
        recovery_id: snapshot.recovery_id,
        last_delivery_sequence: snapshot.last_delivery_sequence,
        layout_revision: snapshot.layout_revision,
    })
    .unwrap();
    let mut last = observer;
    for viewer in 51..=65 {
        let next = match m
            .request(Command::Observe {
                viewer,
                after: None,
            })
            .unwrap()
        {
            Response::Observed(observer) => observer,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            next.epoch,
            last.epoch + 1,
            "actual Err cannot consume an observer epoch or create a hidden view"
        );
        last = next;
    }
    assert_eq!(
        m.request(Command::Observe {
            viewer: 99,
            after: None
        })
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    m.request(Command::Unobserve { observer: last }).unwrap();
    let replacement = match m
        .request(Command::Observe {
            viewer: 99,
            after: None,
        })
        .unwrap()
    {
        Response::Observed(observer) => observer,
        other => panic!("{other:?}"),
    };
    assert_eq!(replacement.epoch, last.epoch + 1);
    m.request(Command::CheckControllerReady { lease }).unwrap();
    let next = match m
        .request(Command::Claim {
            controller: lease.controller,
            takeover: false,
        })
        .unwrap()
    {
        Response::Claimed(lease) => lease,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        next.epoch,
        lease.epoch + 1,
        "the rejected Claim cannot consume a real controller epoch"
    );
    m.host.lease = next;
}
