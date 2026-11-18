#![cfg(windows)]
mod support;
use shellfox_ghostty_vt::{Rgb, TerminalColors};
use shellfox_runtime::*;
use support::Harness;
fn snapshot(host: &mut Harness, observer: ObserverLease) -> StampedRecovery {
    match host
        .request(Command::ObserverRecoverStamped {
            observer,
            capability: AnsiRecoveryCapability::VisibleScreen,
        })
        .unwrap()
    {
        Response::Recovery(snapshot) => snapshot,
        _ => panic!("snapshot expected"),
    }
}
fn accept(host: &mut Harness, observer: ObserverLease, snapshot: &StampedRecovery) {
    host.request(Command::ObserverLayoutAck {
        observer,
        recovery_id: snapshot.recovery_id,
        last_delivery_sequence: snapshot.last_delivery_sequence,
        layout_revision: snapshot.layout_revision,
    })
    .unwrap();
}
#[test]
fn actual_owned_pty_recovery_uses_current_geometry_and_revision_not_frame_or_output_counters() {
    let mut host = Harness::start("--child-idle");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("RUNTIME_IDLE_READY"));
    let observer = match host
        .request(Command::Observe {
            viewer: 81,
            after: None,
        })
        .unwrap()
    {
        Response::Observed(observer) => observer,
        _ => panic!("observer expected"),
    };
    let initial = snapshot(&mut host, observer);
    assert_eq!(
        (initial.cols, initial.rows, initial.layout_revision),
        (80, 24, 1)
    );
    accept(&mut host, observer, &initial);
    host.request(Command::Resize {
        lease: host.lease,
        cols: 120,
        rows: 40,
    })
    .unwrap();
    let resized = snapshot(&mut host, observer);
    assert_eq!(
        (resized.cols, resized.rows, resized.layout_revision),
        (120, 40, 2)
    );
    accept(&mut host, observer, &resized);
    host.request(Command::Colors {
        lease: host.lease,
        colors: Box::new(TerminalColors {
            foreground: Rgb {
                r: 120,
                g: 130,
                b: 140,
            },
            background: Rgb { r: 1, g: 2, b: 3 },
            cursor: None,
            palette: None,
        }),
    })
    .unwrap();
    let recolored = snapshot(&mut host, observer);
    assert_eq!(
        (recolored.cols, recolored.rows, recolored.layout_revision),
        (120, 40, 2),
        "theme/presentation changes do not invent geometry revisions"
    );
    assert_ne!(recolored.recovery_id, resized.recovery_id);
    accept(&mut host, observer, &recolored);
    host.request(Command::Resize {
        lease: host.lease,
        cols: 120,
        rows: 40,
    })
    .unwrap();
    let duplicate = snapshot(&mut host, observer);
    assert_eq!(duplicate.layout_revision, 2);
    accept(&mut host, observer, &duplicate);
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|host| host.exited);
}
