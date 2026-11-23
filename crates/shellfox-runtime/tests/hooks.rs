#![cfg(windows)]
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::*;
use shellfox_terminal::{PtyCommand, PtyConfig};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[derive(Default)]
struct Count {
    calls: AtomicU64,
    bytes: AtomicU64,
    sequence: AtomicU64,
}
impl LiveOutputSink for Count {
    fn output(
        &self,
        _: Identity,
        sequence: u64,
        bytes: &[u8],
        _: Instant,
    ) -> Result<(), LiveOutputFailure> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        self.sequence.store(sequence, Ordering::Release);
        Ok(())
    }
}
struct Failing(AtomicU64);
impl LiveOutputSink for Failing {
    fn output(&self, _: Identity, _: u64, _: &[u8], _: Instant) -> Result<(), LiveOutputFailure> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(LiveOutputFailure)
    }
}
struct Fixture {
    runtime: Runtime,
    root: PathBuf,
}
impl Fixture {
    fn open(mode: &str, sink: Arc<dyn LiveOutputSink>) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("runtime-hook-{}", Generation::new()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-runtime-fixture"));
        let mut config = PtyConfig::isolated(&root);
        config.supervisor = Some(executable.clone());
        config.command = Some(PtyCommand {
            program: executable.into_os_string(),
            args: vec![mode.into()],
        });
        let identity = Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        };
        let runtime = Runtime::start_with_hooks(
            identity,
            config,
            RuntimeHooks {
                live_output: Some(sink),
            },
        )
        .unwrap();
        Self { runtime, root }
    }
    fn until(&self, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !predicate() {
            assert!(
                Instant::now() < deadline,
                "controlled runtime hook timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn call(&self, id: u64, command: Command) -> Response {
        self.runtime
            .try_send(Request {
                id: RequestId(id),
                command,
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(Instant::now() < deadline);
            match self.runtime.try_recv() {
                Ok(Event::Reply { id: reply, result }) if reply == RequestId(id) => {
                    return result.unwrap()
                }
                Ok(Event::Started(Err(error))) => panic!("controlled start failed: {error}"),
                _ => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.runtime.try_send(Request {
            id: RequestId(u64::MAX),
            command: Command::RetryClose,
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.runtime.phase() != Phase::Closed && Instant::now() < deadline {
            while self.runtime.try_recv().is_ok() {}
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.runtime.phase() == Phase::Closed {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
#[test]
fn source_hook_runs_without_controller_and_does_not_observe_parser_generated_input_replies() {
    let counts = Arc::new(Count::default());
    let fixture = Fixture::open("--child-query", counts.clone());
    fixture.until(|| fixture.runtime.phase() == Phase::Closed);
    assert!(counts.calls.load(Ordering::Relaxed) > 0);
    assert_eq!(
        fixture.runtime.live_output_status(),
        LiveOutputStatus::Active
    );
    let lease = match fixture.call(
        1,
        Command::Claim {
            controller: 1,
            takeover: false,
        },
    ) {
        Response::Claimed(lease) => lease,
        _ => panic!("lease expected"),
    };
    let chunks = match fixture.call(2, Command::Replay { lease, after: 0 }) {
        Response::Replay(chunks) => chunks,
        _ => panic!("replay expected"),
    };
    let bytes: usize = chunks.iter().map(|chunk| chunk.bytes.len()).sum();
    assert_eq!(counts.bytes.load(Ordering::Relaxed), bytes as u64);
    let before = counts.calls.load(Ordering::Relaxed);
    fixture.call(
        3,
        Command::Presentation {
            view: PresentationLease::Controller(lease),
            action: PresentationAction::Frame { focused: false },
        },
    );
    fixture.call(
        4,
        Command::Recover {
            lease,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        },
    );
    assert_eq!(counts.calls.load(Ordering::Relaxed), before);
}
#[test]
fn source_hook_is_independent_of_full_event_queue_and_unacknowledged_delivery_credit() {
    let counts = Arc::new(Count::default());
    let fixture = Fixture::open("--child-flood-on-input", counts.clone());
    let lease = match fixture.call(
        1,
        Command::Claim {
            controller: 1,
            takeover: false,
        },
    ) {
        Response::Claimed(lease) => lease,
        _ => panic!("lease expected"),
    };
    assert!(matches!(
        fixture.call(2, Command::Replay { lease, after: 0 }),
        Response::Replay(_)
    ));
    fixture.call(
        3,
        Command::RawInput {
            lease,
            bytes: b"start-flood\r".to_vec(),
        },
    );
    // Only now start the flood. Do not drain or ACK any further event, and never
    // request a GUI frame. The observer itself never waits on a test latch.
    fixture.until(|| fixture.runtime.phase() == Phase::Closed);
    assert!(counts.bytes.load(Ordering::Relaxed) >= 600 * 4096);
    assert!(counts.sequence.load(Ordering::Acquire) > 32);
}
#[test]
fn repeating_idle_fixture_keeps_producing_source_feeds_after_startup_settles() {
    let counts = Arc::new(Count::default());
    let fixture = Fixture::open("--child-idle-escapes-repeat", counts.clone());
    fixture.until(|| {
        fixture.runtime.phase() == Phase::Active && counts.calls.load(Ordering::Relaxed) > 0
    });
    std::thread::sleep(Duration::from_millis(500));
    let before = counts.calls.load(Ordering::Relaxed);
    fixture.until(|| counts.calls.load(Ordering::Relaxed) > before);
    assert_eq!(fixture.runtime.phase(), Phase::Active);
    fixture.call(1, Command::RetryClose);
    fixture.until(|| fixture.runtime.phase() == Phase::Closed);
}
#[derive(Debug)]
struct FixedDiagnostics {
    bytes: usize,
    cr: usize,
    lf: usize,
    esc: usize,
    spaces: usize,
}
fn fixed_diagnostics(mode: &str) -> FixedDiagnostics {
    assert!(matches!(
        mode,
        "--child-idle-escapes-repeat" | "--child-visibility-escapes-repeat"
    ));
    let counts = Arc::new(Count::default());
    let fixture = Fixture::open(mode, counts.clone());
    fixture.until(|| {
        fixture.runtime.phase() == Phase::Active && counts.sequence.load(Ordering::Acquire) > 0
    });
    std::thread::sleep(Duration::from_millis(2500));
    let after = counts.sequence.load(Ordering::Acquire);
    fixture.until(|| counts.sequence.load(Ordering::Acquire) >= after + 4);
    let lease = match fixture.call(
        1,
        Command::Claim {
            controller: 1,
            takeover: false,
        },
    ) {
        Response::Claimed(lease) => lease,
        _ => panic!("controlled diagnostic lease expected"),
    };
    let chunks = match fixture.call(2, Command::Replay { lease, after }) {
        Response::Replay(chunks) => chunks,
        _ => panic!("controlled diagnostic replay expected"),
    };
    // Read only this fixed owned fixture's existing bounded journal, not a raw
    // cache in the live observer. Return coarse counts, never raw bytes/text.
    let mut result = FixedDiagnostics {
        bytes: 0,
        cr: 0,
        lf: 0,
        esc: 0,
        spaces: 0,
    };
    for byte in chunks.iter().flat_map(|chunk| &chunk.bytes) {
        result.bytes += 1;
        match *byte {
            b'\r' => result.cr += 1,
            b'\n' => result.lf += 1,
            27 => result.esc += 1,
            b' ' => result.spaces += 1,
            _ => {}
        }
    }
    assert!(result.bytes > 0 && result.bytes <= 128 * 1024);
    result
}
#[test]
fn controlled_conpty_fixtures_report_actual_post_startup_bytes_without_a_quiet_assumption() {
    for mode in [
        "--child-idle-escapes-repeat",
        "--child-visibility-escapes-repeat",
    ] {
        let actual = fixed_diagnostics(mode);
        eprintln!("fixed-owned-fixture {mode} post-startup counts: {actual:?}");
        // Classification belongs to the existing Service policy. In particular,
        // CR/LF produced by ConPTY must not be suppressed or relabelled as idle.
        assert!(actual.esc > 0);
    }
}
#[test]
fn supplier_failure_disables_only_the_sink_while_the_owned_idle_shell_stays_active() {
    let failing = Arc::new(Failing(AtomicU64::new(0)));
    let fixture = Fixture::open("--child-escape-idle", failing.clone());
    fixture.until(|| fixture.runtime.live_output_status() == LiveOutputStatus::Unavailable);
    assert_eq!(fixture.runtime.phase(), Phase::Active);
    assert_eq!(failing.0.load(Ordering::Relaxed), 1);
    fixture.call(1, Command::RetryClose);
    fixture.until(|| fixture.runtime.phase() == Phase::Closed);
}
