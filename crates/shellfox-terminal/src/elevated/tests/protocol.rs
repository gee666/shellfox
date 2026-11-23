use super::super::*;
use super::support::*;
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

#[test]
fn malformed_duplicate_unknown_and_oversized_frames_fail_closed() {
    let cases = vec![
        br#"{"type":"ready","pid":789,"birth":"123"}"#.to_vec(),
        br#"{"type":"data","data":"eA==","command":"evil"}"#.to_vec(),
        br#"{"type":"data","data":"eB=="}"#.to_vec(),
        br#"{"type":"ack","id":0}"#.to_vec(),
        br#"{"type":"exit","exitCode":1.5}"#.to_vec(),
        br#"{"type":"error","code":"unknown","message":"hidden"}"#.to_vec(),
        br#"{"type":"data","type":"exit","data":"","exitCode":0}"#.to_vec(),
        br#"{"type":"ack","id":1,"id":1}"#.to_vec(),
        vec![b'x'; MAX_FRAME + 1],
        serde_json::to_vec(&json!({"type":"error","code":"failed","message":"🙂".repeat(501)}))
            .unwrap(),
    ];
    for frame in cases {
        let (mut channel, io) = active();
        raw(&io, frame);
        assert_eq!(
            channel.poll(generation()),
            Err(AdapterError::InvalidProtocol)
        );
        assert!(io.0.borrow().disconnected);
        assert_eq!(channel.phase(), Phase::Unconfirmed);
        assert!(!channel.cleanup_confirmed());
        send(&io, json!({"type":"exit", "exitCode":0}));
        assert_eq!(
            channel.poll(generation()),
            Err(AdapterError::CleanupUnconfirmed)
        );
    }
    let (mut channel, io) = start();
    send(&io, json!({"type":"data", "data":"eA=="}));
    assert_eq!(
        channel.poll(generation()),
        Err(AdapterError::InvalidProtocol)
    );
}

#[test]
fn startup_error_keeps_only_known_diagnostics_and_never_confirms_cleanup() {
    for (code, message, expected) in [
        ("cancelled", "Windows administrator approval was denied or dismissed.", AdapterError::ApprovalDenied),
        ("failed", "Elevated helper started but failed at creating the PowerShell ConPTY.", AdapterError::StartupFailed(StartupStage::CreatePowerShell)),
        ("failed", "Elevated helper started but failed at reading private-input.", AdapterError::HelperFailed),
        ("failed", "Windows administrator approval was denied or dismissed.", AdapterError::HelperFailed),
    ] {
        let (mut channel, io) = start();
        send(&io, json!({"type":"error", "code":code, "message":message}));
        assert_eq!(channel.poll(generation()).unwrap(), Some(Event::Failed(expected)));
        assert!(!channel.cleanup_confirmed());
        assert!(!expected.to_string().contains("private-input"));
        eof(&io);
        assert_eq!(channel.poll(generation()), Err(AdapterError::CleanupUnconfirmed));
    }
}

#[test]
fn cleanup_proof_is_distinct_from_transport_eof_and_not_revoked_by_trailing_garbage() {
    let (mut channel, io) = active();
    send(&io, json!({"type":"exit", "exitCode":17}));
    assert_eq!(
        channel.poll(generation()).unwrap(),
        Some(Event::ExitConfirmed(17))
    );
    assert!(channel.cleanup_confirmed());
    send(&io, json!({"type":"data", "data":"eA=="}));
    assert_eq!(
        channel.poll(generation()),
        Err(AdapterError::InvalidProtocol)
    );
    assert!(channel.cleanup_confirmed());
}

#[test]
fn launch_fields_and_runtime_hooks_are_validated_before_any_process_can_start() {
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for key in [
        "DOTNET_STARTUP_HOOKS",
        "ComPlus_Version",
        "CORECLR_PROFILER",
        "cor_enable_profiling",
        "bad=key",
        "bad\0key",
    ] {
        assert!(Launch::fixture(
            cwd.clone(),
            BTreeMap::from([(key.into(), "synthetic".into())]),
            80,
            24
        )
        .is_err());
    }
    assert!(Launch::fixture(
        cwd.clone(),
        BTreeMap::from([("Path".into(), "a".into()), ("PATH".into(), "b".into())]),
        80,
        24
    )
    .is_err());
    assert!(Launch::fixture(
        cwd.clone(),
        BTreeMap::from([("CUSTOM".into(), "\0".into())]),
        80,
        24
    )
    .is_err());
    assert!(Launch::fixture(
        cwd.clone(),
        BTreeMap::from([("CUSTOM".into(), "🙂".repeat(16384))]),
        80,
        24
    )
    .is_err());
    assert!(Launch::fixture("relative".into(), BTreeMap::new(), 80, 24).is_err());
    assert!(Launch::fixture(cwd.clone(), BTreeMap::new(), 80, 1).is_err());
    let oversized = Launch::fixture(
        cwd.join("雪".repeat(32000)),
        BTreeMap::from([("CUSTOM".into(), "\x01".repeat(32000))]),
        80,
        24,
    )
    .unwrap();
    assert!(oversized.prepare().is_err());
}

#[test]
fn probe_accepts_only_the_v1_line() {
    use super::super::launch::parse_probe;
    parse_probe(b"shellfox-elevated-conpty-v1\r\n").unwrap();
    for line in [
        &b"shellfox-elevated-conpty-v2"[..],
        br#"{"protocol":"shellfox-elevated-isolated","version":1,"shell":"windows-powershell","noProfile":true}"#,
    ] {
        assert_eq!(parse_probe(line), Err(AdapterError::InvalidProtocol));
    }
}

#[test]
fn streaming_lines_are_bounded_across_arbitrary_chunks_and_partial_eof() {
    let wire = b"{\"type\":\"ack\",\"id\":1}\n{\"type\":\"exit\",\"exitCode\":0}\n";
    let mut decoder = LineDecoder::default();
    let mut lines = Vec::new();
    for bytes in wire.chunks(3) {
        decoder
            .feed(bytes, |frame| {
                if let HelperFrame::Line(line) = frame {
                    lines.push(line);
                }
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(lines.len(), 2);
    let mut ended = false;
    decoder
        .finish(|frame| {
            ended = matches!(frame, HelperFrame::Eof);
            Ok(())
        })
        .unwrap();
    assert!(ended);
    assert_eq!(
        decoder.feed(b"late", |_| Ok(())),
        Err(AdapterError::InvalidProtocol)
    );

    let mut decoder = LineDecoder::default();
    decoder.feed(b"partial", |_| Ok(())).unwrap();
    assert_eq!(
        decoder.finish(|_| Ok(())),
        Err(AdapterError::InvalidProtocol)
    );
    let mut decoder = LineDecoder::default();
    decoder.feed(&vec![b'x'; MAX_FRAME], |_| Ok(())).unwrap();
    assert_eq!(
        decoder.feed(b"x", |_| Ok(())),
        Err(AdapterError::InvalidProtocol)
    );
    assert_eq!(
        decoder.feed(b"\n", |_| Ok(())),
        Err(AdapterError::InvalidProtocol)
    );
}
