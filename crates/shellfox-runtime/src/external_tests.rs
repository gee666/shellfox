use super::*;
#[test]
fn full_retains_exact_global_chunk_and_does_not_advance_source_sequence() {
    let (mut source, mut owner) = channel();
    for sequence in 1..=EXTERNAL_FEED_CAPACITY as u64 {
        source
            .try_feed(ExternalChunk {
                sequence: SourceSequence(sequence),
                stream: ExternalStream::Stdout,
                bytes: vec![b'x'; EXTERNAL_CHUNK_BYTES],
            })
            .unwrap();
    }
    let chunk = ExternalChunk {
        sequence: SourceSequence(9),
        stream: ExternalStream::Stderr,
        bytes: b"held-exactly".to_vec(),
    };
    let pointer = chunk.bytes.as_ptr();
    let held = match source.try_feed(chunk) {
        Err(ExternalFeedError::Full(chunk)) => chunk,
        _ => panic!("expected full"),
    };
    assert_eq!(held.bytes.as_ptr(), pointer);
    assert_eq!(held.sequence, SourceSequence(9));
    assert_eq!(held.stream, ExternalStream::Stderr);
    assert_eq!(source.queued, SourceSequence(8));
    let first = owner.receiver.as_ref().unwrap().try_recv().unwrap();
    assert!(matches!(
        first,
        ExternalMessage::Data(ExternalChunk {
            sequence: SourceSequence(1),
            ..
        })
    ));
    source.try_feed(held).unwrap();
    assert_eq!(source.queued, SourceSequence(9));
    assert!(matches!(
        source.try_finish(SourceSequence(9)),
        Err(ExternalFinishError::Full)
    ));
    owner.receiver.take();
    let closed = ExternalChunk {
        sequence: SourceSequence(10),
        stream: ExternalStream::Stdout,
        bytes: b"not-lost".to_vec(),
    };
    let pointer = closed.bytes.as_ptr();
    assert!(
        matches!(source.try_feed(closed), Err(ExternalFeedError::Closed(chunk)) if chunk.bytes.as_ptr() == pointer && chunk.sequence == SourceSequence(10))
    );
}
#[test]
fn abort_consumes_sender_without_consuming_exact_full_pending_event() {
    let (mut source, owner) = channel();
    for sequence in 1..=EXTERNAL_FEED_CAPACITY as u64 {
        source
            .try_feed(ExternalChunk {
                sequence: SourceSequence(sequence),
                stream: ExternalStream::Stdout,
                bytes: vec![b'x'],
            })
            .unwrap();
    }
    let pending = ExternalChunk {
        sequence: SourceSequence(9),
        stream: ExternalStream::Stderr,
        bytes: b"pending-owned-by-relay".to_vec(),
    };
    let pointer = pending.bytes.as_ptr();
    let pending = match source.try_feed(pending) {
        Err(ExternalFeedError::Full(chunk)) => chunk,
        _ => panic!("full expected"),
    };
    let identity = source.identity;
    let evidence = source.abort(ExternalAbortReason::ProducerFailed);
    assert_eq!(evidence.identity, identity);
    assert_eq!(evidence.last_queued, SourceSequence(8));
    assert_eq!(pending.sequence, SourceSequence(9));
    assert_eq!(pending.stream, ExternalStream::Stderr);
    assert_eq!(pending.bytes.as_ptr(), pointer);
    let receiver = owner.receiver.as_ref().unwrap();
    for sequence in 1..=8 {
        assert!(
            matches!(receiver.try_recv(), Ok(ExternalMessage::Data(ExternalChunk { sequence: SourceSequence(actual), .. })) if actual == sequence)
        );
    }
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(owner.status.lock().unwrap().abort, Some(evidence));
    assert!(!owner.status.lock().unwrap().finished);
}
#[test]
fn bounded_feed_rejects_empty_oversize_and_unbounded_capacity_without_consuming_them() {
    let (mut source, _owner) = channel();
    for bytes in [Vec::new(), vec![b'x'; EXTERNAL_CHUNK_BYTES + 1], {
        let mut v = Vec::with_capacity(EXTERNAL_CHUNK_BYTES + 1);
        v.push(b'x');
        v
    }] {
        let original = bytes.as_ptr();
        let chunk = ExternalChunk {
            sequence: SourceSequence(1),
            stream: ExternalStream::Stdout,
            bytes,
        };
        assert!(
            matches!(source.try_feed(chunk), Err(ExternalFeedError::Invalid { chunk, .. }) if chunk.bytes.as_ptr() == original)
        );
        assert_eq!(source.queued, SourceSequence(0));
    }
}
