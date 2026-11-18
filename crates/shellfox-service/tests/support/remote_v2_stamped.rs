use super::{
    delivery::{DeliveryFailure, ObserverDelivery},
    identity, request,
};
use shellfox_protocol::observer_v2 as wire;
use shellfox_runtime::*;

#[test]
fn same_sequence_and_revision_never_make_an_old_opaque_recovery_ack_current() {
    let runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
    let mut view = None;
    let Response::Observed(observer) = request(
        &runtime,
        1,
        Command::Observe {
            viewer: 1,
            after: None,
        },
        &mut view,
    )
    .unwrap() else {
        panic!("observer")
    };
    view = Some(ObserverDelivery::new(runtime.identity(), observer).unwrap());
    let Response::Recovery(first) = request(
        &runtime,
        2,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
        &mut view,
    )
    .unwrap() else {
        panic!("recovery")
    };
    let old_command = Command::ObserverLayoutAck {
        observer,
        recovery_id: first.recovery_id,
        last_delivery_sequence: first.last_delivery_sequence,
        layout_revision: first.layout_revision,
    };
    let first_wire = view
        .as_mut()
        .unwrap()
        .recover(Response::Recovery(first))
        .unwrap();
    let second = request(
        &runtime,
        3,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
        &mut view,
    )
    .unwrap();
    let second_wire = view.as_mut().unwrap().recover(second).unwrap();
    assert_eq!(first_wire.last_sequence, second_wire.last_sequence);
    assert_eq!(first_wire.layout_revision, second_wire.layout_revision);
    assert_ne!(first_wire.recovery_id, second_wire.recovery_id);
    let reference = view.as_ref().unwrap().reference().clone();
    assert!(view
        .as_ref()
        .unwrap()
        .ack_command(
            &reference,
            &wire::Ack::Recovery {
                recovery_id: first_wire.recovery_id,
                sequence: first_wire.last_sequence
            }
        )
        .is_err());
    assert!(request(&runtime, 4, old_command, &mut view).is_err());
    let ack = wire::Ack::Recovery {
        recovery_id: second_wire.recovery_id,
        sequence: second_wire.last_sequence,
    };
    let command = view
        .as_ref()
        .unwrap()
        .ack_command(&reference, &ack)
        .unwrap();
    let response = request(&runtime, 5, command, &mut view).unwrap();
    view.as_mut()
        .unwrap()
        .commit_ack(&reference, &ack, &response)
        .unwrap();
}

#[test]
fn actual_canonical_resize_requires_full_stamped_recovery_not_a_wire_layout_grant() {
    let runtime = Runtime::start_external(identity(), ExternalConfig::default()).unwrap();
    let mut view = None;
    let Response::Observed(observer) = request(
        &runtime,
        1,
        Command::Observe {
            viewer: 1,
            after: None,
        },
        &mut view,
    )
    .unwrap() else {
        panic!("observer")
    };
    view = Some(ObserverDelivery::new(runtime.identity(), observer).unwrap());
    let response = request(
        &runtime,
        2,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
        &mut view,
    )
    .unwrap();
    let first = view.as_mut().unwrap().recover(response).unwrap();
    let reference = view.as_ref().unwrap().reference().clone();
    let ack = wire::Ack::Recovery {
        recovery_id: first.recovery_id,
        sequence: first.last_sequence,
    };
    let command = view
        .as_ref()
        .unwrap()
        .ack_command(&reference, &ack)
        .unwrap();
    let response = request(&runtime, 3, command, &mut view).unwrap();
    view.as_mut()
        .unwrap()
        .commit_ack(&reference, &ack, &response)
        .unwrap();
    // This trusted fixture changes canonical geometry directly. No V2 backend
    // method emits ResizeExternal, PTY resize, or any standalone layout grant.
    request(
        &runtime,
        4,
        Command::Presentation {
            view: PresentationLease::Observer(observer),
            action: PresentationAction::ResizeExternal {
                cols: 100,
                rows: 30,
            },
        },
        &mut view,
    )
    .unwrap();
    let response = request(
        &runtime,
        5,
        Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
        &mut view,
    )
    .unwrap();
    let layouts = view.as_mut().unwrap().poll(1024, 10).unwrap();
    assert!(matches!(
        &layouts[..],
        [wire::Event::Layout {
            cols: 100,
            rows: 30,
            recovery_required: true,
            ..
        }]
    ));
    assert_eq!(
        view.as_ref()
            .unwrap()
            .ack_command(&reference, &ack)
            .unwrap_err(),
        DeliveryFailure::RecoveryRequired
    );
    let next = view.as_mut().unwrap().recover(response).unwrap();
    assert_eq!((next.cols, next.rows), (100, 30));
    assert!(next.layout_revision > first.layout_revision);
    assert_eq!(
        next.last_sequence, first.last_sequence,
        "geometry revision is not a byte counter"
    );
    assert!(view.as_mut().unwrap().poll(1024, 10).unwrap().is_empty());
}
