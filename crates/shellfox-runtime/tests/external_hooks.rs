#[path = "support/external.rs"]
mod common;
use common::*;
use shellfox_runtime::*;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};
#[derive(Default)]
struct PublicationGate {
    bound: AtomicBool,
    callbacks: AtomicU64,
    published_bytes: AtomicU64,
}
impl LiveOutputSink for PublicationGate {
    fn output(
        &self,
        _: Identity,
        _: u64,
        bytes: &[u8],
        _: Instant,
    ) -> Result<(), LiveOutputFailure> {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        if self.bound.load(Ordering::Acquire) {
            self.published_bytes
                .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }
        Ok(())
    }
}
#[test]
fn explicit_external_sink_gets_only_new_display_bytes_and_final_flush_not_replay_or_frames() {
    let (default, mut raw) = source();
    assert_eq!(default.live_output_status(), LiveOutputStatus::Disabled);
    finish(&mut raw, 0);
    wait(&default, || default.external_status().unwrap().finished);
    let gate = Arc::new(PublicationGate::default());
    let mut runtime = Runtime::start_external_with_policy_and_hooks(
        identity(),
        ExternalConfig::default(),
        ExternalDisplayPolicy::LegacyAutomationV1,
        RuntimeHooks {
            live_output: Some(gate.clone()),
        },
    )
    .unwrap();
    let mut source = runtime.take_source().unwrap();
    feed(&mut source, 1, ExternalStream::Stdout, vec![0xf0, 0x9f]);
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(1)
    });
    assert_eq!(
        gate.callbacks.load(Ordering::Relaxed),
        0,
        "pending transport UTF8 is not displayed output"
    );
    feed(&mut source, 2, ExternalStream::Stderr, b"A\n".to_vec());
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(2)
    });
    assert_eq!(gate.callbacks.load(Ordering::Relaxed), 1);
    assert_eq!(
        gate.published_bytes.load(Ordering::Relaxed),
        0,
        "unbound publication gate creates no metadata touch"
    );
    gate.bound.store(true, Ordering::Release);
    feed(&mut source, 3, ExternalStream::Stdout, vec![0x91, 0xa9]);
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(3)
    });
    assert_eq!(gate.published_bytes.load(Ordering::Relaxed), 4);
    let observer = observe(&runtime, 1, 71, None);
    assert!(text(&frame(&runtime, 2, observer)).contains("👩"));
    request(
        &runtime,
        3,
        Command::ObserverRecover {
            observer,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
    )
    .unwrap();
    assert_eq!(
        gate.published_bytes.load(Ordering::Relaxed),
        4,
        "history/frame/recovery must not re-touch"
    );
    feed(&mut source, 4, ExternalStream::Stderr, vec![0xe2]);
    wait(&runtime, || {
        runtime.external_status().unwrap().source_sequence == SourceSequence(4)
    });
    finish(&mut source, 4);
    wait(&runtime, || runtime.external_status().unwrap().finished);
    assert_eq!(
        gate.published_bytes.load(Ordering::Relaxed),
        7,
        "final decoder flush is new display, not raw process exit proof"
    );
    assert!(!runtime.external_status().unwrap().closed);
}
