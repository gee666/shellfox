use super::*;
use crate::ErrorCode;

fn attach(hub: &mut ObserverHub, journal: &Delivery, viewer: u64) -> ObserverLease {
    hub.observe(viewer, Some(0), journal).unwrap()
}
fn round(hub: &mut ObserverHub, journal: &Delivery, closed: bool) -> Vec<ObserverEvent> {
    let mut events = Vec::new();
    hub.pump(journal, closed, |event| {
        events.push(event);
        true
    });
    events
}
fn bounded(hub: &ObserverHub) {
    assert!(hub.views.len() <= MAX_OBSERVERS);
    for view in &hub.views {
        assert!(view.acked <= view.sent);
        assert!(view.pending.len() <= PENDING_CHUNKS);
        assert!(view.pending_bytes <= CREDIT_BYTES);
        assert_eq!(
            view.pending_bytes,
            view.pending.iter().map(|p| p.1).sum::<usize>()
        );
    }
}

#[test]
fn observers_never_claim_takeover_or_reset_controller_delivery() {
    for owned in [false, true] {
        let mut journal = Delivery::new();
        journal.append(vec![1; 8192]).unwrap();
        let controller = owned.then(|| journal.claim(5, false).unwrap());
        if let Some(lease) = controller {
            journal.replay(lease, 0).unwrap();
        }
        let before = (
            journal.lease(),
            journal.last_sent(),
            journal.first_sequence(),
        );
        let mut hub = ObserverHub::new();
        let lease = attach(&mut hub, &journal, 5);
        assert_eq!(round(&mut hub, &journal, false).len(), 1);
        assert_eq!(round(&mut hub, &journal, false).len(), 1);
        hub.acknowledge(lease, 2).unwrap();
        assert_eq!(hub.recover(lease, &journal).unwrap(), 2);
        hub.acknowledge(lease, 2).unwrap();
        hub.unobserve(lease).unwrap();
        assert_eq!(
            (
                journal.lease(),
                journal.last_sent(),
                journal.first_sequence()
            ),
            before
        );
        if let Some(lease) = controller {
            journal.check(lease).unwrap();
            journal.append(vec![2]).unwrap();
            assert_eq!(journal.next_live().unwrap().sequence, 3);
        }
        bounded(&hub);
    }
}

#[test]
fn duplicate_viewers_fence_old_epochs_without_unbounded_slots() {
    let journal = Delivery::new();
    let mut hub = ObserverHub::new();
    let old = attach(&mut hub, &journal, 1);
    let current = attach(&mut hub, &journal, 1);
    assert!(current.epoch > old.epoch);
    assert!(hub.check(old).is_err());
    assert!(hub.acknowledge(old, 0).is_err());
    assert!(hub.replay(old, 0, &journal).is_err());
    assert!(hub.recover(old, &journal).is_err());
    assert!(hub.unobserve(old).is_err());
    for viewer in 2..=16 {
        attach(&mut hub, &journal, viewer);
    }
    assert!(hub.observe(17, None, &journal).is_err());
    assert!(hub.observe(1, Some(1), &journal).is_err());
    hub.check(current).unwrap();
    let wrong = ObserverLease {
        viewer: 2,
        epoch: current.epoch,
    };
    assert!(hub.acknowledge(wrong, 0).is_err());
    hub.unobserve(current).unwrap();
    assert!(hub.unobserve(current).is_err());
    attach(&mut hub, &journal, 17);
    bounded(&hub);
}

#[test]
fn epoch_exhaustion_is_atomic_and_never_reuses_a_lease() {
    let journal = Delivery::new();
    let mut hub = ObserverHub::new();
    let lease = attach(&mut hub, &journal, 3);
    hub.epoch = u64::MAX;
    assert!(hub.observe(3, None, &journal).is_err());
    assert!(hub.observe(4, None, &journal).is_err());
    assert_eq!(hub.views.len(), 1);
    hub.check(lease).unwrap();
}

#[test]
fn fresh_none_needs_replay_and_replay_cannot_reset_credit() {
    let mut journal = Delivery::new();
    journal.append(vec![1; 8193]).unwrap();
    let mut hub = ObserverHub::new();
    let lease = hub.observe(1, None, &journal).unwrap();
    assert!(round(&mut hub, &journal, true).is_empty());
    assert!(hub.replay(lease, 4, &journal).is_err());
    hub.replay(lease, 0, &journal).unwrap();
    round(&mut hub, &journal, false);
    assert!(hub.replay(lease, 0, &journal).is_err());
    hub.replay(lease, 1, &journal).unwrap();
    assert_eq!(hub.views[0].pending_bytes, 4096);
    assert!(hub.acknowledge(lease, 2).is_err());
    assert_eq!(hub.views[0].sent, 1);
    bounded(&hub);
}

#[test]
fn byte_credit_and_acks_are_independent_per_observer() {
    let mut journal = Delivery::new();
    journal.append(vec![1; 40 * 4096]).unwrap();
    let mut hub = ObserverHub::new();
    let first = attach(&mut hub, &journal, 1);
    let second = attach(&mut hub, &journal, 2);
    for _ in 0..32 {
        assert_eq!(round(&mut hub, &journal, false).len(), 2);
    }
    assert!(round(&mut hub, &journal, false).is_empty());
    assert_eq!(hub.views[0].pending_bytes, CREDIT_BYTES);
    hub.acknowledge(first, 32).unwrap();
    for _ in 0..8 {
        assert!(
            matches!(round(&mut hub, &journal, false).as_slice(), [ObserverEvent::Output { lease, .. }] if *lease == first)
        );
    }
    assert_eq!(hub.views[1].sent, 32);
    assert_eq!(hub.views[1].pending_bytes, CREDIT_BYTES);
    assert!(hub.acknowledge(second, 40).is_err());
    hub.acknowledge(second, 3).unwrap();
    hub.acknowledge(second, 3).unwrap();
    assert!(hub.acknowledge(second, 2).is_err());
    hub.recover(first, &journal).unwrap();
    assert_eq!(hub.views[1].sent, 32);
    assert_eq!(hub.views[1].pending.len(), 29);
    assert!(hub.views[1].recovery_pending.is_none());
    bounded(&hub);
}

#[test]
fn tiny_chunks_stop_at_metadata_limit_not_only_byte_limit() {
    let mut journal = Delivery::new();
    for _ in 0..300 {
        journal.append(vec![1]).unwrap();
    }
    let mut hub = ObserverHub::new();
    let lease = attach(&mut hub, &journal, 1);
    for _ in 0..256 {
        round(&mut hub, &journal, false);
    }
    assert!(round(&mut hub, &journal, false).is_empty());
    assert_eq!(hub.views[0].pending.len(), 256);
    hub.acknowledge(lease, 100).unwrap();
    for _ in 0..44 {
        round(&mut hub, &journal, false);
    }
    assert_eq!(hub.views[0].sent, 300);
    assert_eq!(hub.views[0].pending.len(), 200);
    bounded(&hub);
}

#[test]
fn full_event_queue_does_not_commit_data_or_allow_future_ack() {
    let mut journal = Delivery::new();
    journal.append(vec![1]).unwrap();
    let mut hub = ObserverHub::new();
    let lease = attach(&mut hub, &journal, 1);
    assert!(!hub.pump(&journal, false, |_| false));
    assert_eq!((hub.views[0].sent, hub.views[0].pending_bytes), (0, 0));
    assert!(hub.acknowledge(lease, 1).is_err());
    assert!(
        matches!(round(&mut hub, &journal, false).as_slice(), [ObserverEvent::Output { lease: got, chunk }] if *got == lease && chunk.sequence == 1)
    );
    hub.acknowledge(lease, 1).unwrap();
    bounded(&hub);
}

#[test]
fn one_slot_event_sink_stays_fair_across_rejected_rounds_for_sixteen_observers() {
    let mut journal = Delivery::new();
    journal.append(vec![1; 8192]).unwrap();
    let mut hub = ObserverHub::new();
    for viewer in 0..16 {
        attach(&mut hub, &journal, viewer);
    }
    let mut delivered = Vec::new();
    for _ in 0..32 {
        assert!(!hub.pump(&journal, false, |_| false));
        let mut free = true;
        hub.pump(&journal, false, |event| {
            if !free {
                return false;
            }
            free = false;
            if let ObserverEvent::Output { lease, chunk } = event {
                delivered.push((lease.viewer, chunk.sequence));
            } else {
                panic!("unexpected observer event");
            }
            true
        });
    }
    assert_eq!(
        delivered,
        (1..=2)
            .flat_map(|sequence| (0..16).map(move |viewer| (viewer, sequence)))
            .collect::<Vec<_>>()
    );
    bounded(&hub);
}

#[test]
fn history_loss_retries_notification_then_blocks_until_recovery() {
    let mut journal = Delivery::new();
    let controller = journal.claim(9, false).unwrap();
    let mut hub = ObserverHub::new();
    let lease = attach(&mut hub, &journal, 1);
    journal.append(vec![1; 1024 * 1024 + 4096]).unwrap();
    assert!(!hub.pump(&journal, true, |_| false));
    assert!(!hub.views[0].loss_reported);
    assert!(
        matches!(round(&mut hub, &journal, true).as_slice(), [ObserverEvent::ReplayLost { lease: got, last_sequence: 257 }] if *got == lease)
    );
    assert!(round(&mut hub, &journal, true).is_empty());
    hub.acknowledge(lease, 0).unwrap();
    assert_eq!(
        hub.replay(lease, 0, &journal).unwrap_err().code,
        ErrorCode::ReplayLost
    );
    assert!(hub.replay(lease, 257, &journal).is_err());
    let barrier = hub.recover(lease, &journal).unwrap();
    assert_eq!(journal.lease(), Some(controller));
    assert_eq!(journal.last_sent(), 0);
    assert_eq!(journal.first_sequence(), Some(2));
    journal.append(vec![2]).unwrap();
    hub.acknowledge(lease, barrier - 1).unwrap();
    assert!(round(&mut hub, &journal, true).is_empty());
    assert!(hub.acknowledge(lease, barrier + 1).is_err());
    hub.acknowledge(lease, barrier).unwrap();
    assert!(
        matches!(round(&mut hub, &journal, true).as_slice(), [ObserverEvent::Output { chunk, .. }] if chunk.sequence == barrier + 1)
    );
    bounded(&hub);
}

#[test]
fn expired_resume_and_credit_stall_both_report_loss_without_tail_skips() {
    let mut journal = Delivery::new();
    let mut hub = ObserverHub::new();
    let first = attach(&mut hub, &journal, 1);
    journal.append(vec![1; CREDIT_BYTES]).unwrap();
    for _ in 0..32 {
        round(&mut hub, &journal, false);
    }
    journal.append(vec![2; 1024 * 1024 + 4096]).unwrap();
    let resumed = hub.observe(2, Some(0), &journal).unwrap();
    let events = round(&mut hub, &journal, false);
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| matches!(event, ObserverEvent::ReplayLost { lease, .. } if *lease == first || *lease == resumed)));
    assert_eq!(hub.views[0].sent, 32);
    assert_eq!(hub.views[0].pending_bytes, CREDIT_BYTES);
    assert_eq!(hub.views[1].sent, 0);
    bounded(&hub);
}

#[test]
fn recovery_zero_and_replacement_barriers_require_exact_ack() {
    let mut journal = Delivery::new();
    let mut hub = ObserverHub::new();
    let lease = hub.observe(1, None, &journal).unwrap();
    assert_eq!(hub.recover(lease, &journal).unwrap(), 0);
    journal.append(vec![1]).unwrap();
    assert!(round(&mut hub, &journal, true).is_empty());
    hub.acknowledge(lease, 0).unwrap();
    round(&mut hub, &journal, false);
    hub.recover(lease, &journal).unwrap();
    journal.append(vec![2]).unwrap();
    hub.recover(lease, &journal).unwrap();
    hub.acknowledge(lease, 1).unwrap();
    assert!(round(&mut hub, &journal, true).is_empty());
    hub.acknowledge(lease, 2).unwrap();
    assert!(matches!(
        round(&mut hub, &journal, true).as_slice(),
        [ObserverEvent::Exited {
            last_sequence: 2,
            ..
        }]
    ));
    bounded(&hub);
}

#[test]
fn each_observer_exits_only_after_its_final_output_has_been_queued() {
    let mut journal = Delivery::new();
    journal.append(vec![1; 8192]).unwrap();
    let mut hub = ObserverHub::new();
    let first = attach(&mut hub, &journal, 1);
    let second = hub.observe(2, Some(1), &journal).unwrap();
    assert!(round(&mut hub, &journal, true)
        .iter()
        .all(|e| matches!(e, ObserverEvent::Output { .. })));
    let events = round(&mut hub, &journal, true);
    assert!(events.iter().any(
        |e| matches!(e, ObserverEvent::Exited { lease, last_sequence: 2 } if *lease == second)
    ));
    assert!(events.iter().any(|e| matches!(e, ObserverEvent::Output { lease, chunk } if *lease == first && chunk.sequence == 2)));
    assert!(!hub.pump(&journal, true, |_| false));
    assert!(!hub.views[0].exit_sent);
    assert!(
        matches!(round(&mut hub, &journal, true).as_slice(), [ObserverEvent::Exited { lease, last_sequence: 2 }] if *lease == first)
    );
    assert!(round(&mut hub, &journal, true).is_empty());
}

#[test]
fn eviction_of_sent_history_and_controller_takeover_do_not_disturb_observers() {
    let mut journal = Delivery::new();
    journal.claim(1, false).unwrap();
    let mut hub = ObserverHub::new();
    let lease = attach(&mut hub, &journal, 3);
    for sequence in 1..=600 {
        journal.append(vec![1; 4096]).unwrap();
        assert!(
            matches!(round(&mut hub, &journal, false).as_slice(), [ObserverEvent::Output { chunk, .. }] if chunk.sequence == sequence)
        );
        hub.acknowledge(lease, sequence).unwrap();
    }
    let controller = journal.claim(2, true).unwrap();
    assert_eq!(journal.lease(), Some(controller));
    hub.check(lease).unwrap();
    assert!(round(&mut hub, &journal, false).is_empty());
    bounded(&hub);
}
