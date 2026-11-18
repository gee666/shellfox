#![cfg(windows)]
mod support;
use shellfox_runtime::*;
use support::Harness;

fn observed(response: Response) -> ObserverLease {
    match response {
        Response::Observed(lease) => lease,
        response => panic!("unexpected observer reply {response:?}"),
    }
}
#[test]
fn readonly_subscribers_do_not_evict_controller_and_have_independent_epochs_and_acks() {
    let mut host = Harness::start("--child-echo");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("RUNTIME_READY"));
    let control = host.lease;
    let first = observed(
        host.request(Command::Observe {
            viewer: 10,
            after: Some(0),
        })
        .unwrap(),
    );
    let second = observed(
        host.request(Command::Observe {
            viewer: 20,
            after: Some(0),
        })
        .unwrap(),
    );
    let replacement = observed(
        host.request(Command::Observe {
            viewer: 10,
            after: Some(0),
        })
        .unwrap(),
    );
    assert!(replacement.epoch > first.epoch);
    assert_eq!(host.lease, control);
    for command in [
        Command::ObserverAck {
            observer: first,
            sequence: 0,
        },
        Command::ObserverReplay {
            observer: first,
            after: 0,
        },
        Command::Unobserve { observer: first },
        Command::ObserverAck {
            observer: ObserverLease {
                viewer: replacement.viewer,
                epoch: second.epoch,
            },
            sequence: 0,
        },
    ] {
        assert_eq!(
            host.request(command).unwrap_err().code,
            ErrorCode::ControlRequired
        );
    }
    assert_eq!(
        host.request(Command::ObserverAck {
            observer: second,
            sequence: u64::MAX
        })
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    // A read-only delivery identity is not a terminal-control identity.
    assert_eq!(
        host.request(Command::RawInput {
            lease: Lease {
                controller: second.viewer,
                epoch: second.epoch
            },
            bytes: b"wrong\r".to_vec(),
        })
        .unwrap_err()
        .code,
        ErrorCode::ControlRequired
    );
    let response = host
        .request(Command::ObserverRecover {
            observer: second,
            capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
        })
        .unwrap();
    let Response::Ansi {
        capability,
        last_sequence,
        ..
    } = response
    else {
        panic!("expected recovery");
    };
    assert_eq!(capability, AnsiRecoveryCapability::FormattedActiveBufferV1);
    host.request(Command::ObserverAck {
        observer: second,
        sequence: last_sequence,
    })
    .unwrap();
    // The original controller still writes after readonly attach/recovery/fencing.
    host.request(Command::RawInput {
        lease: control,
        bytes: b"observer-safe\r".to_vec(),
    })
    .unwrap();
    host.until(|h| {
        h.exited && h.observer_exits.contains(&replacement) && h.observer_exits.contains(&second)
    });
    for observer in [replacement, second] {
        let bytes: Vec<_> = host
            .observer_output
            .iter()
            .filter(|(lease, _)| *lease == observer)
            .flat_map(|(_, chunk)| chunk.bytes.iter().copied())
            .collect();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("RUNTIME_ECHO=observer-safe"),
            "{observer:?}: {text}"
        );
        assert!(!text.contains("RUNTIME_ECHO=wrong"));
    }
    assert_eq!(host.runtime.phase(), Phase::Closed);
}
#[test]
fn fresh_readonly_attachment_requires_replay_and_can_detach_without_closing_shell() {
    let mut host = Harness::start("--child-idle");
    host.until(|h| String::from_utf8_lossy(&h.output).contains("RUNTIME_IDLE_READY"));
    let observer = observed(
        host.request(Command::Observe {
            viewer: 30,
            after: None,
        })
        .unwrap(),
    );
    host.request(Command::Screen { lease: host.lease }).unwrap();
    assert!(!host
        .observer_output
        .iter()
        .any(|(lease, _)| *lease == observer));
    host.request(Command::ObserverReplay { observer, after: 0 })
        .unwrap();
    host.until(|h| {
        h.observer_output
            .iter()
            .any(|(lease, _)| *lease == observer)
    });
    host.request(Command::Unobserve { observer }).unwrap();
    assert_eq!(host.runtime.phase(), Phase::Active);
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|h| h.exited);
}
