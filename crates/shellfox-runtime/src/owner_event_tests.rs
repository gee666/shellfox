use super::*;

#[test]
fn a_one_slot_channel_is_shared_fairly_by_controller_and_observers() {
    let mut owner = test_support::owner();
    let control = owner.delivery.claim(1, false).unwrap();
    owner.delivery.replay(control, 0).unwrap();
    owner.delivery.append(vec![b'x'; 128 * 1024]).unwrap();
    let first = owner
        .observers
        .observe(10, Some(0), &owner.delivery)
        .unwrap();
    let second = owner
        .observers
        .observe(20, Some(0), &owner.delivery)
        .unwrap();
    let (events, received) = crate::owner_wake::channel(&owner.shared, 1);
    let mut counts = [0usize; 3];
    for _ in 0..32 {
        owner.emit_events(&events);
        match received.try_recv().expect("one fair delivery per round") {
            Event::Output { lease, .. } => {
                assert_eq!(lease, control);
                counts[0] += 1;
            }
            Event::ObserverOutput { observer, .. } if observer == first => counts[1] += 1,
            Event::ObserverOutput { observer, .. } if observer == second => counts[2] += 1,
            event => panic!("unexpected scheduler event: {event:?}"),
        }
    }
    assert!(
        counts[0] > 0 && counts[1] > 0 && counts[2] > 0,
        "{counts:?}"
    );
    assert!(
        counts[1].abs_diff(counts[2]) <= 1,
        "observers must rotate: {counts:?}"
    );
    assert_eq!(
        counts[0],
        counts[1] + counts[2],
        "controller and observer-first rounds alternate"
    );
}

#[test]
fn queue_full_cannot_advance_controller_sent_or_allow_future_ack() {
    let mut delivery = Delivery::new();
    let lease = delivery.claim(1, false).unwrap();
    delivery.replay(lease, 0).unwrap();
    delivery.append(b"queued once".to_vec()).unwrap();
    assert!(!delivery.pump_live(|_| false));
    assert_eq!(delivery.last_sent(), 0);
    assert!(delivery.acknowledge(lease, 1).is_err());
    let mut received = None;
    assert!(delivery.pump_live(|chunk| {
        received = Some(chunk);
        true
    }));
    assert_eq!(received.unwrap().sequence, 1);
    assert_eq!(delivery.last_sent(), 1);
    delivery.acknowledge(lease, 1).unwrap();
    assert!(!delivery.pump_live(|_| panic!("no duplicate live output")));
}
