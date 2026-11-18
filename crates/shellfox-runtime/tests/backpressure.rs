#![cfg(windows)]
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::*;
use shellfox_terminal::{PtyCommand, PtyConfig};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
fn slow_viewer_cannot_block_process_cleanup_or_queue_unbounded_frames() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .join(format!("runtime-slow-view-{}", Generation::new()));
    std::fs::create_dir_all(&root).unwrap();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-runtime-fixture"));
    let mut config = PtyConfig::new(&root);
    config.supervisor = Some(exe.clone());
    config.command = Some(PtyCommand {
        program: exe.into_os_string(),
        args: vec!["--child-flood".into()],
    });
    let runtime = Runtime::start(
        Identity {
            session_id: SessionId::new(),
            tab_id: TabId::new(),
            generation: Generation::new(),
        },
        config,
    )
    .unwrap();
    runtime
        .try_send(Request {
            id: RequestId(1),
            command: Command::Claim {
                controller: 1,
                takeover: false,
            },
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let lease = loop {
        assert!(Instant::now() < deadline);
        match runtime.try_recv() {
            Ok(Event::Reply {
                result: Ok(Response::Claimed(lease)),
                ..
            }) => break lease,
            Ok(Event::Started(Err(error))) => panic!("startup failed: {error}"),
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    runtime
        .try_send(Request {
            id: RequestId(2),
            command: Command::Replay { lease, after: 0 },
        })
        .unwrap();
    // Do not receive events, take frames, or ACK. Both channels and byte credit
    // can fill while the owner keeps parsing into one coalesced screen slot.
    while runtime.phase() != Phase::Closed {
        assert!(Instant::now() < deadline, "slow viewer prevented cleanup");
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame = loop {
        assert!(Instant::now() < deadline, "final screen was not published");
        if let Some(frame) = runtime.take_screen() {
            let text: String = frame
                .screen
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect();
            if text.contains("RUNTIME_FLOOD_FINAL") {
                break frame;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(runtime.take_screen().is_none(), "there is no frame backlog");
    let mut delivered_bytes = 0;
    let mut delivered_sequence = 0;
    let mut exited = false;
    let mut replay_lost = false;
    // Drain only the bounded already-queued notifications. Never ACK them.
    let drain_until = Instant::now() + Duration::from_millis(100);
    while Instant::now() < drain_until {
        match runtime.try_recv() {
            Ok(Event::Output {
                lease: event_lease,
                chunk,
            }) => {
                assert_eq!(event_lease, lease);
                delivered_bytes += chunk.bytes.len();
                delivered_sequence = chunk.sequence;
            }
            Ok(Event::Reply {
                result: Ok(Response::Replay(chunks)),
                ..
            }) => {
                for chunk in chunks {
                    delivered_bytes += chunk.bytes.len();
                    delivered_sequence = chunk.sequence;
                }
            }
            Ok(Event::Reply {
                result:
                    Err(RuntimeError {
                        code: ErrorCode::ReplayLost,
                        ..
                    }),
                ..
            })
            | Ok(Event::ReplayLost { .. }) => replay_lost = true,
            Ok(Event::Exited { .. }) => exited = true,
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    assert!(
        delivered_bytes <= 128 * 1024,
        "credit was implicitly acknowledged"
    );
    assert!(
        frame.sequence > delivered_sequence,
        "screen acceptance is not byte delivery"
    );
    assert!(
        !exited,
        "exit must follow final sequence delivery, not merely cleanup"
    );
    assert!(
        replay_lost,
        "a stalled viewer must learn replay was overwritten"
    );
    drop(runtime);
    std::fs::remove_dir_all(root).unwrap();
}
