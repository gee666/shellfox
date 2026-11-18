use super::*;
use shellfox_runtime::*;
use std::time::Duration;

fn wait(runtime: &Runtime, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(Instant::now() < deadline);
        assert!(!runtime.external_status().unwrap().failed);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn original_external_runtime_hooks_gate_every_live_display_chunk_without_any_observer_credit() {
    let identity = identity();
    let gate = Arc::new(AutomationActivity::new(identity));
    let mut owner = owner();
    let mut runtime = Runtime::start_external_with_policy_and_hooks(
        identity,
        ExternalConfig::default(),
        ExternalDisplayPolicy::LegacyAutomationV1,
        gate.hooks(),
    )
    .unwrap();
    let mut source = runtime.take_source().unwrap();
    source
        .try_feed(ExternalChunk {
            sequence: SourceSequence(1),
            stream: ExternalStream::Stdout,
            bytes: b"hidden\r\n".to_vec(),
        })
        .unwrap();
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    assert!(owner.take_live_updates().is_empty());
    let (data, host) = host();
    assert!(host.snapshot().unwrap().sessions.is_empty());
    gate.publish(&mut owner, OsStr::new("real-raw-program"), || {
        host.publish_reserved_session(publication(identity, data.path()), None)
            .map_err(Into::into)
    })
    .unwrap();
    let sink = owner.live_output_sink(identity).unwrap();
    assert!(
        sink.take_update().is_none(),
        "Show never replays hidden output into activity"
    );
    for sequence in 2..=33 {
        source
            .try_feed(ExternalChunk {
                sequence: SourceSequence(sequence),
                stream: ExternalStream::Stdout,
                bytes: b"post-publication\r\n".to_vec(),
            })
            .unwrap();
        wait(&runtime, || {
            runtime.external_status().unwrap().source_sequence == SourceSequence(sequence)
        });
        let update = sink
            .take_update()
            .expect("each accepted live display chunk updates the same book");
        assert_eq!(
            update.last_sequence,
            runtime.external_status().unwrap().last_delivery_sequence.0
        );
        assert!(update.output_at_ms.is_some());
        assert!(update.available);
    }
    source.try_finish(SourceSequence(33)).unwrap();
    wait(&runtime, || runtime.external_status().unwrap().finished);
    assert!(
        gate.take_exit_touch().is_none(),
        "source Finish alone is not actual raw exit evidence"
    );
    assert!(
        !runtime.external_status().unwrap().closed,
        "source-ended phase is not the original parser-thread join or raw process cleanup"
    );
    assert!(host.session_creation_facts().unwrap().is_empty());
}
#[test]
fn actual_late_show_after_ordered_finish_has_no_historical_activity_or_exit_touch() {
    let identity = identity();
    let gate = Arc::new(AutomationActivity::new(identity));
    let mut owner = owner();
    let mut runtime =
        Runtime::start_external_with_hooks(identity, ExternalConfig::default(), gate.hooks())
            .unwrap();
    let mut source = runtime.take_source().unwrap();
    source
        .try_feed(ExternalChunk {
            sequence: SourceSequence(1),
            stream: ExternalStream::Stdout,
            bytes: b"old output".to_vec(),
        })
        .unwrap();
    source.try_finish(SourceSequence(1)).unwrap();
    wait(&runtime, || runtime.external_status().unwrap().finished);
    let (data, host) = host();
    gate.publish(&mut owner, OsStr::new("raw-program"), || {
        host.publish_reserved_session(publication(identity, data.path()), None)
            .map_err(Into::into)
    })
    .unwrap();
    assert!(owner.take_live_updates().is_empty());
    assert!(gate.take_exit_touch().is_none());
    assert_eq!(
        owner.live_output_sink(identity).unwrap().status(),
        (false, true)
    );
}
