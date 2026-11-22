#![cfg(windows)]
mod support;
use support::Harness;
#[path = "support/controller.rs"]
mod controller;
use controller::*;
#[path = "support/controller_grant_audit.rs"]
mod grant_audit;
#[path = "support/controller_input_limits.rs"]
mod input_limits;
#[path = "support/controller_keys.rs"]
mod keys;
#[path = "support/controller_mouse.rs"]
mod mouse;
use shellfox_runtime::*;
use std::{sync::atomic::Ordering, time::Duration};
#[test]
fn actual_owned_pty_recovery_gates_output_binary_resize_and_upload_readiness_until_exact_consumption(
) {
    let mut m = Manager::start("--child-idle-escapes-repeat", "\x1b[");
    let lease = m.lease();
    let first = m.recover();
    let latest = m.recover();
    assert_ne!(first.recovery_id, latest.recovery_id);
    assert_eq!(
        (latest.cols, latest.rows, latest.layout_revision),
        (80, 24, 1)
    );
    m.events.clear();
    let parsed = m.probe.output.load(Ordering::SeqCst);
    m.poll_for(Duration::from_millis(250));
    assert!(
        m.probe.output.load(Ordering::SeqCst) > parsed,
        "real source still feeds the sole parser while delivery waits"
    );
    assert!(!m
        .events
        .iter()
        .any(|event| matches!(event, Event::Output { .. })));
    assert_eq!(m.ack(&first).unwrap_err().code, ErrorCode::Validation);
    for command in [
        Command::CheckControllerReady { lease },
        Command::InteractiveBytes {
            lease,
            bytes: vec![0, 128, 255],
        },
        Command::Resize {
            lease,
            cols: 100,
            rows: 30,
        },
    ] {
        assert_eq!(
            m.request(command).unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
    }
    assert!(m.probe.input.lock().unwrap().is_empty());
    assert_eq!(
        m.request(Command::Ack {
            lease,
            sequence: latest.last_delivery_sequence
        })
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    m.ack(&latest).unwrap();
    m.request(Command::CheckControllerReady { lease }).unwrap();
    m.until(|m|m.events.iter().any(|event|matches!(event,Event::Output{chunk,..}if chunk.sequence>latest.last_delivery_sequence)));
    let bytes = vec![0x80, 0, 0xff, b'\r'];
    let input_id = RequestId(m.host.next);
    assert!(matches!(
        m.request(Command::InteractiveBytes {
            lease,
            bytes: bytes.clone()
        })
        .unwrap(),
        Response::Input(InputReceipt {
            accepted_bytes: 4,
            kind: InputKind::Raw
        })
    ));
    assert_eq!(
        m.probe.input.lock().unwrap().as_slice(),
        &[(input_id, InputKind::Raw, bytes)]
    );
    // This proves exact bytes into the actual PTY writer, not arbitrary shell interpretation.
    m.request(Command::Resize {
        lease,
        cols: 100,
        rows: 30,
    })
    .unwrap();
    m.until(|m| {
        m.events.iter().any(|event| {
            matches!(
                event,
                Event::ControllerLayout {
                    layout: LayoutStamp {
                        cols: 100,
                        rows: 30,
                        layout_revision: 2,
                        ..
                    },
                    ..
                }
            )
        })
    });
    assert_eq!(
        m.request(Command::CheckControllerReady { lease })
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    let resized = m.recover();
    assert_eq!(
        (resized.cols, resized.rows, resized.layout_revision),
        (100, 30, 2)
    );
    m.ack(&resized).unwrap();
    m.request(Command::Resize {
        lease,
        cols: 100,
        rows: 30,
    })
    .unwrap();
    let same = m.recover();
    assert_eq!(same.layout_revision, 2);
    m.ack(&same).unwrap();
}
#[test]
fn actual_takeover_and_release_revoke_old_real_epochs_without_killing_owned_pty_or_adding_observer()
{
    let mut m = Manager {
        host: Harness::start("--child-idle"),
        events: Vec::new(),
        probe: Default::default(),
    };
    m.host
        .until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_IDLE_READY"));
    let old = m.lease();
    let first = m.recover();
    m.ack(&first).unwrap();
    m.events.clear();
    let replacement = match m
        .request(Command::Claim {
            controller: 2,
            takeover: true,
        })
        .unwrap()
    {
        Response::Claimed(lease) => lease,
        other => panic!("{other:?}"),
    };
    assert!(replacement.epoch > old.epoch);
    assert!(
        matches!(m.events.first(),Some(Event::ControllerRevoked{lease,reason:ControllerRevocation::Replaced})if *lease==old)
    );
    assert!(!m
        .events
        .iter()
        .any(|event| matches!(event, Event::ObserverOutput { .. })
            || matches!(event,Event::Output{lease,..}if *lease==replacement)));
    assert_eq!(
        m.request(Command::InteractiveBytes {
            lease: old,
            bytes: b"stale".to_vec()
        })
        .unwrap_err()
        .code,
        ErrorCode::ControlRequired
    );
    m.host.lease = replacement;
    let latest = m.recover();
    assert_eq!(m.ack(&first).unwrap_err().code, ErrorCode::Validation);
    m.ack(&latest).unwrap();
    m.events.clear();
    m.request(Command::ReleaseControl { lease: replacement })
        .unwrap();
    assert_eq!(m.host.runtime.phase(), Phase::Active);
    assert!(
        matches!(m.events.first(),Some(Event::ControllerRevoked{lease,reason:ControllerRevocation::Released})if *lease==replacement)
    );
    assert_eq!(
        m.request(Command::CheckControllerReady { lease: replacement })
            .unwrap_err()
            .code,
        ErrorCode::ControlRequired
    );
    let next = match m
        .request(Command::Claim {
            controller: 3,
            takeover: false,
        })
        .unwrap()
    {
        Response::Claimed(lease) => lease,
        _ => panic!(),
    };
    assert!(next.epoch > replacement.epoch);
    m.host.lease = next;
    assert_eq!(
        m.request(Command::InteractiveBytes {
            lease: next,
            bytes: b"before-recovery".to_vec()
        })
        .unwrap_err()
        .code,
        ErrorCode::RecoveryRequired
    );
    let restored = m.recover();
    m.ack(&restored).unwrap();
    m.request(Command::CheckControllerReady { lease: next })
        .unwrap();
}
