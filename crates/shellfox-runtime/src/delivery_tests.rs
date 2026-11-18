use super::*;

fn lease(delivery: &Delivery) -> Lease {
    delivery.lease().unwrap()
}
fn ack(delivery: &mut Delivery, sequence: u64) -> Result<(), String> {
    delivery.acknowledge(lease(delivery), sequence)
}
fn replay(delivery: &mut Delivery, after: u64) -> Result<Vec<Chunk>, String> {
    delivery.replay(lease(delivery), after)
}
fn bounded(delivery: &Delivery) {
    assert!(delivery.history.len() <= REPLAY_CHUNKS);
    assert!(delivery.history_bytes <= REPLAY_BYTES);
    assert!(delivery.pending.len() <= PENDING_CHUNKS);
    assert!(delivery.pending_bytes <= CREDIT_BYTES);
    assert_eq!(
        delivery.history_bytes,
        delivery
            .history
            .iter()
            .map(|c| c.bytes.len())
            .sum::<usize>()
    );
    assert_eq!(
        delivery.pending_bytes,
        delivery.pending.iter().map(|p| p.1).sum::<usize>()
    );
    assert!(delivery.acked <= delivery.sent && delivery.sent <= delivery.sequence);
}

#[test]
fn claims_fence_all_access_and_denial_preserves_credit() {
    let mut d = Delivery::new();
    assert!(d.lease().is_none());
    assert!(d
        .check(Lease {
            controller: 1,
            epoch: 0
        })
        .is_err());
    let first = d.claim(1, false).unwrap();
    assert_eq!(first.epoch, 1);
    d.append(vec![1]).unwrap();
    replay(&mut d, 0).unwrap();
    assert!(d.claim(2, false).is_err());
    assert_eq!((d.epoch, d.sent, d.pending_bytes), (1, 1, 1));
    assert!(d
        .check(Lease {
            controller: 2,
            epoch: 1
        })
        .is_err());
    let refreshed = d.claim(1, false).unwrap();
    assert_eq!(refreshed.epoch, 2);
    assert!(d.check(first).is_err());
    assert_eq!(
        (d.sent, d.acked, d.pending_bytes, d.pending.len()),
        (0, 0, 0, 0)
    );
    let next = d.claim(2, true).unwrap();
    assert_eq!(next.epoch, 3);
    assert!(d.acknowledge(refreshed, 0).is_err());
    assert!(d
        .replay(
            Lease {
                controller: 1,
                epoch: 2
            },
            0
        )
        .is_err());
    d.claim(1, true).unwrap();
    assert!(d.check(next).is_err());
    assert_eq!(d.lease().unwrap().epoch, 4);
    assert_eq!(replay(&mut d, 0).unwrap()[0].bytes, [1]);
}

#[test]
fn append_splits_raw_bytes_and_empty_input_does_not_advance() {
    let mut d = Delivery::new();
    let bytes: Vec<u8> = (0..CHUNK_BYTES * 2 + 1).map(|n| n as u8).collect();
    assert_eq!(d.append(bytes.clone()).unwrap(), 3);
    assert_eq!(d.append(Vec::new()).unwrap(), 3);
    assert!(d.next_live().is_none());
    d.claim(0, false).unwrap();
    let chunks = replay(&mut d, 0).unwrap();
    assert_eq!(
        chunks.iter().map(|c| c.sequence).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        chunks.iter().map(|c| c.bytes.len()).collect::<Vec<_>>(),
        [4096, 4096, 1]
    );
    assert_eq!(
        chunks.into_iter().flat_map(|c| c.bytes).collect::<Vec<_>>(),
        bytes
    );
    bounded(&d);
}

#[test]
fn live_byte_credit_and_partial_duplicate_ack_are_exact() {
    let mut d = Delivery::new();
    d.claim(1, false).unwrap();
    assert!(replay(&mut d, 0).unwrap().is_empty());
    d.append(vec![255; CHUNK_BYTES * 40]).unwrap();
    for sequence in 1..=32 {
        assert_eq!(d.next_live().unwrap().sequence, sequence);
    }
    assert!(d.next_live().is_none());
    assert_eq!((d.last_sent(), d.current_sequence()), (32, 40));
    assert!(ack(&mut d, 33).is_err());
    ack(&mut d, 3).unwrap();
    for sequence in 33..=35 {
        assert_eq!(d.next_live().unwrap().sequence, sequence);
    }
    ack(&mut d, 3).unwrap();
    assert!(ack(&mut d, 2).is_err());
    assert!(d.next_live().is_none());
    ack(&mut d, 35).unwrap();
    assert_eq!(replay(&mut d, 35).unwrap().len(), 5);
    bounded(&d);
}

#[test]
fn replay_and_live_share_byte_credit_without_implicit_ack_or_skip() {
    let mut d = Delivery::new();
    d.claim(1, false).unwrap();
    d.append(vec![42; CHUNK_BYTES * 40]).unwrap();
    assert!(replay(&mut d, 41).is_err());
    assert!(!d.initialized);
    assert_eq!(replay(&mut d, 0).unwrap().len(), 32);
    assert!(replay(&mut d, 0).is_err());
    assert!(replay(&mut d, 40).is_err());
    assert!(replay(&mut d, 32).unwrap().is_empty());
    assert!(d.next_live().is_none());
    ack(&mut d, 1).unwrap();
    assert_eq!(replay(&mut d, 32).unwrap()[0].sequence, 33);
    assert!(d.next_live().is_none());
    bounded(&d);
}

#[test]
fn tiny_chunks_hit_pending_limit_and_replay_continues_in_order() {
    let mut d = Delivery::new();
    d.claim(1, false).unwrap();
    for _ in 0..300 {
        d.append(vec![0]).unwrap();
    }
    assert_eq!(replay(&mut d, 0).unwrap().len(), PENDING_CHUNKS);
    assert!(d.next_live().is_none());
    assert!(ack(&mut d, 300).is_err());
    ack(&mut d, 128).unwrap();
    let chunks = replay(&mut d, 256).unwrap();
    assert_eq!(chunks.first().unwrap().sequence, 257);
    assert_eq!(chunks.last().unwrap().sequence, 300);
    assert_eq!(d.pending.len(), 172);
    bounded(&d);
}

#[test]
fn history_byte_and_chunk_bounds_require_ansi_recovery_not_a_tail() {
    for (count, size, oldest) in [(257, CHUNK_BYTES, 2), (513, 1, 2)] {
        let mut d = Delivery::new();
        d.claim(1, false).unwrap();
        for _ in 0..count {
            d.append(vec![b'x'; size]).unwrap();
        }
        assert_eq!(d.history.front().unwrap().sequence, oldest);
        assert!(replay(&mut d, 0)
            .unwrap_err()
            .contains("ANSI screen recovery"));
        assert_eq!((d.sent, d.acked, d.pending_bytes), (0, 0, 0));
        assert!(!d.initialized);
        assert!(d.next_live().is_none());
        assert_eq!(replay(&mut d, oldest - 1).unwrap()[0].sequence, oldest);
        bounded(&d);
    }
}

#[test]
fn stalled_live_delivery_never_skips_evicted_unsent_output() {
    let mut d = Delivery::new();
    d.claim(1, false).unwrap();
    d.append(vec![1; CREDIT_BYTES]).unwrap();
    assert_eq!(replay(&mut d, 0).unwrap().len(), 32);
    d.append(vec![2; REPLAY_BYTES + CHUNK_BYTES]).unwrap();
    ack(&mut d, 32).unwrap();
    assert!(d.next_live().is_none());
    assert_eq!(d.sent, 32);
    assert!(ack(&mut d, 33).is_err());
    assert!(replay(&mut d, 32)
        .unwrap_err()
        .contains("ANSI screen recovery"));
    d.claim(1, false).unwrap();
    let current = d.current_sequence();
    // The caller restores an ANSI screen at this cursor before resuming.
    assert_eq!(d.recover(lease(&d)).unwrap(), current);
    d.append(b"\x1b[1;1Hlive".to_vec()).unwrap();
    assert!(d.next_live().is_none());
    assert!(replay(&mut d, current).unwrap().is_empty());
    ack(&mut d, current).unwrap();
    assert_eq!(d.next_live().unwrap().sequence, current + 1);
    bounded(&d);
}

#[test]
fn fresh_claim_requires_initial_replay_or_acked_recovery_even_at_zero() {
    let mut d = Delivery::new();
    let stale = d.claim(1, false).unwrap();
    assert_eq!(d.recover(lease(&d)).unwrap(), 0);
    d.append(vec![1]).unwrap();
    assert!(d.next_live().is_none());
    assert!(replay(&mut d, 0).unwrap().is_empty());
    ack(&mut d, 0).unwrap();
    assert_eq!(d.next_live().unwrap().sequence, 1);
    d.claim(1, false).unwrap();
    assert!(d.recover(stale).is_err());
    assert!(d.next_live().is_none());
    assert_eq!(replay(&mut d, 0).unwrap()[0].sequence, 1);
    bounded(&d);
}

#[test]
fn recovery_replaces_credit_without_implicitly_acknowledging_its_barrier() {
    let mut d = Delivery::new();
    d.claim(1, false).unwrap();
    d.append(vec![1; CREDIT_BYTES]).unwrap();
    replay(&mut d, 0).unwrap();
    assert_eq!(d.recover(lease(&d)).unwrap(), 32);
    assert_eq!((d.pending_bytes, d.pending.len(), d.acked), (0, 0, 0));
    d.append(vec![2; CHUNK_BYTES]).unwrap();
    ack(&mut d, 31).unwrap();
    assert!(d.next_live().is_none());
    assert!(replay(&mut d, 32).unwrap().is_empty());
    assert!(ack(&mut d, 33).is_err());
    assert_eq!(d.recover(lease(&d)).unwrap(), 33);
    ack(&mut d, 32).unwrap();
    d.append(vec![3]).unwrap();
    assert!(d.next_live().is_none());
    ack(&mut d, 33).unwrap();
    assert_eq!(d.next_live().unwrap().sequence, 34);
    bounded(&d);
}

#[test]
fn replay_loss_notifies_once_without_advancing_or_freeing_credit() {
    let mut d = Delivery::new();
    d.claim(1, false).unwrap();
    d.append(vec![1; CREDIT_BYTES]).unwrap();
    replay(&mut d, 0).unwrap();
    d.append(vec![2; REPLAY_BYTES + CHUNK_BYTES]).unwrap();
    assert!(d.take_replay_lost());
    assert!(!d.take_replay_lost());
    assert_eq!((d.sent, d.acked, d.pending_bytes), (32, 0, CREDIT_BYTES));
    d.claim(1, false).unwrap();
    assert!(!d.take_replay_lost());
    assert!(replay(&mut d, 0).is_err());
    let barrier = d.recover(lease(&d)).unwrap();
    d.append(vec![3; REPLAY_BYTES + CHUNK_BYTES]).unwrap();
    assert!(!d.take_replay_lost());
    ack(&mut d, barrier).unwrap();
    assert!(d.take_replay_lost());
    assert!(!d.take_replay_lost());
    let barrier = d.recover(lease(&d)).unwrap();
    ack(&mut d, barrier).unwrap();
    assert!(!d.take_replay_lost());
    d.append(vec![4; REPLAY_BYTES + CHUNK_BYTES]).unwrap();
    assert!(d.take_replay_lost());
    bounded(&d);
}

#[test]
fn eviction_of_already_sent_history_is_not_a_live_gap() {
    let mut d = Delivery::new();
    d.claim(1, false).unwrap();
    replay(&mut d, 0).unwrap();
    for sequence in 1..=600 {
        d.append(vec![1; CHUNK_BYTES]).unwrap();
        assert_eq!(d.next_live().unwrap().sequence, sequence);
        assert!(!d.take_replay_lost());
        ack(&mut d, sequence).unwrap();
    }
    assert!(d.history.front().unwrap().sequence > 1);
    assert!(d.next_live().is_none());
    assert!(!d.take_replay_lost());
    bounded(&d);
}

#[test]
fn counter_exhaustion_is_atomic_and_never_reuses_an_epoch_or_sequence() {
    let mut d = Delivery::new();
    d.epoch = u64::MAX;
    assert!(d.claim(1, true).is_err());
    assert!(d.lease().is_none());
    d.epoch = 0;
    d.claim(1, false).unwrap();
    d.sequence = u64::MAX - 1;
    assert!(d.append(vec![0; CHUNK_BYTES + 1]).is_err());
    assert_eq!(d.current_sequence(), u64::MAX - 1);
    assert!(d.history.is_empty());
    assert_eq!(d.append(vec![1]).unwrap(), u64::MAX);
    assert_eq!(replay(&mut d, u64::MAX - 1).unwrap()[0].sequence, u64::MAX);
    ack(&mut d, u64::MAX).unwrap();
    assert!(d.next_live().is_none());
    assert!(d.append(vec![2]).is_err());
    assert_eq!(d.append(Vec::new()).unwrap(), u64::MAX);
    bounded(&d);
}
