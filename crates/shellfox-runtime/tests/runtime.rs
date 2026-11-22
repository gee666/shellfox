#![cfg(windows)]
mod support;
use shellfox_core::Generation;
use shellfox_runtime::*;
use std::path::PathBuf;
use support::Harness;

#[test]
fn final_bytes_precede_exit_and_headless_owner_retains_final_screen() {
    let mut host = Harness::start("--child-final");
    host.until(|h| h.exited);
    assert!(String::from_utf8_lossy(&host.output).contains("RUNTIME_FINAL_BYTES"));
    assert_eq!(host.runtime.phase(), Phase::Closed);
    let frame = host
        .last_screen
        .take()
        .or_else(|| host.runtime.take_screen())
        .unwrap();
    let text: String = frame.screen.cells.iter().map(|c| c.text.as_str()).collect();
    assert!(text.contains("RUNTIME_FINAL_BYTES"));
    assert_eq!(frame.identity, host.runtime.identity());
}
#[test]
fn authoritative_parser_answers_queries_without_any_view_parser() {
    let mut host = Harness::start("--child-query");
    host.until(|h| h.exited);
    let text = String::from_utf8_lossy(&host.output);
    assert!(text.contains("RUNTIME_QUERY_FINAL"), "{text}");
    assert_eq!(text.matches("QUERY_REPLY=").count(), 1, "{text}");
    assert!(
        text.contains("27, 91, 49, 59, 49, 82"),
        "expected one ESC[1;1R query reply: {text}"
    );
}
#[test]
fn takeover_fences_old_input_and_ack_then_owner_encodes_paste() {
    let mut host = Harness::start("--child-echo");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("RUNTIME_READY"));
    let old = host.lease;
    let denied = host
        .request(Command::Claim {
            controller: 2,
            takeover: false,
        })
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::ControlRequired);
    host.lease = match host
        .request(Command::Claim {
            controller: 2,
            takeover: true,
        })
        .unwrap()
    {
        Response::Claimed(lease) => lease,
        _ => panic!("expected new lease"),
    };
    assert!(host.lease.epoch > old.epoch);
    for command in [
        Command::RawInput {
            lease: old,
            bytes: b"wrong\r".to_vec(),
        },
        Command::Ack {
            lease: old,
            sequence: 0,
        },
    ] {
        assert_eq!(
            host.request(command).unwrap_err().code,
            ErrorCode::ControlRequired
        );
    }
    let response = host
        .request(Command::RecoverAnsi { lease: host.lease })
        .unwrap();
    let Response::Ansi {
        last_sequence,
        bytes,
        capability,
    } = response
    else {
        panic!("expected ANSI");
    };
    assert_eq!(capability, AnsiRecoveryCapability::VisibleScreen);
    assert!(std::str::from_utf8(&bytes).is_ok());
    host.request(Command::Ack {
        lease: host.lease,
        sequence: last_sequence,
    })
    .unwrap();
    host.request(Command::Paste {
        lease: host.lease,
        text: "typed by owner".into(),
    })
    .unwrap();
    host.request(Command::Key {
        lease: host.lease,
        event: shellfox_ghostty_vt::key::KeyEvent::press(
            shellfox_ghostty_vt::key::PhysicalKey::Enter,
            shellfox_ghostty_vt::key::KeyModifiers::NONE,
        ),
    })
    .unwrap();
    host.until(|h| h.exited);
    let text = String::from_utf8_lossy(&host.output);
    assert!(text.contains("RUNTIME_ECHO=typed by owner"), "{text}");
    assert!(!text.contains("RUNTIME_ECHO=wrong"));
}
#[test]
fn resize_colors_selection_and_close_use_owner_thread() {
    let mut host = Harness::start("--child-idle");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("RUNTIME_IDLE_READY"));
    host.request(Command::Resize {
        lease: host.lease,
        cols: 100,
        rows: 30,
    })
    .unwrap();
    host.request(Command::Colors {
        lease: host.lease,
        colors: Box::new(shellfox_ghostty_vt::TerminalColors {
            foreground: shellfox_ghostty_vt::Rgb {
                r: 210,
                g: 210,
                b: 210,
            },
            background: shellfox_ghostty_vt::Rgb {
                r: 16,
                g: 16,
                b: 16,
            },
            cursor: None,
            palette: None,
        }),
    })
    .unwrap();
    host.request(Command::Selection {
        lease: host.lease,
        selection: Some(shellfox_ghostty_vt::Selection::All),
    })
    .unwrap();
    let Response::Text(Some(text)) = host.request(Command::Copy { lease: host.lease }).unwrap()
    else {
        panic!("expected copied text");
    };
    assert!(text.contains("RUNTIME_IDLE_READY"));
    host.request(Command::Screen { lease: host.lease }).unwrap();
    host.until(|h| h.last_screen.as_ref().is_some_and(|s| s.screen.cols == 100));
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|h| h.exited);
    assert_eq!(host.runtime.phase(), Phase::Closed);
}
#[test]
fn formatted_recovery_requires_explicit_capability_and_an_exact_ack_barrier() {
    let mut host = Harness::start("--child-idle");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("RUNTIME_IDLE_READY"));
    let response = host
        .request(Command::Recover {
            lease: host.lease,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        })
        .unwrap();
    let Response::Ansi {
        capability,
        bytes,
        last_sequence,
    } = response
    else {
        panic!("expected negotiated ANSI");
    };
    assert_eq!(capability, AnsiRecoveryCapability::FormattedActiveBufferV1);
    assert!(bytes.starts_with(b"\x18\x1bc"));
    assert!(bytes.len() <= 4 * 1024 * 1024);
    host.request(Command::Ack {
        lease: host.lease,
        sequence: last_sequence,
    })
    .unwrap();
    assert_eq!(host.runtime.phase(), Phase::Active);
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|h| h.exited);
}
#[test]
fn high_dpi_grid_uses_configured_limits_without_closing_the_shell() {
    let mut host = Harness::start("--child-idle");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("RUNTIME_IDLE_READY"));
    host.request(Command::Resize {
        lease: host.lease,
        cols: 500,
        rows: 250,
    })
    .unwrap();
    host.until(|h| {
        h.last_screen
            .as_ref()
            .is_some_and(|frame| frame.screen.cols == 500 && frame.screen.rows == 250)
    });
    let error = host
        .runtime
        .try_send(Request {
            id: RequestId(999),
            command: Command::Resize {
                lease: host.lease,
                cols: 1024,
                rows: 1024,
            },
        })
        .unwrap_err();
    assert!(matches!(
        error,
        SubmitError::Invalid(RuntimeError {
            code: ErrorCode::Validation,
            ..
        })
    ));
    assert_eq!(host.runtime.phase(), Phase::Active);
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|h| h.exited);
}
#[test]
fn oversized_intents_fail_before_bounded_queue_owns_them() {
    let host = Harness::start("--child-idle");
    let error = host
        .runtime
        .try_send(Request {
            id: RequestId(999),
            command: Command::RawInput {
                lease: host.lease,
                bytes: vec![0; MAX_INPUT_BYTES + 1],
            },
        })
        .unwrap_err();
    assert!(matches!(
        error,
        SubmitError::Invalid(RuntimeError {
            code: ErrorCode::Validation,
            ..
        })
    ));
}
#[test]
fn own_supervisor_is_usable_from_headless_executable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .join(format!("runtime-headless-{}", Generation::new()));
    std::fs::create_dir_all(&root).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shellfox-runtime-fixture"))
        .args([std::ffi::OsStr::new("--headless-check"), root.as_os_str()])
        .output()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("RUNTIME_FINAL_BYTES"));
}
