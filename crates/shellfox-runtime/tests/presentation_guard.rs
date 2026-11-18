#![cfg(windows)]
mod support;
use shellfox_ghostty_vt::{Selection, ViewportPoint};
use shellfox_runtime::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::Harness;
fn frame(host: &mut Harness, view: PresentationLease) -> Arc<ViewFrame> {
    match host
        .request(Command::Presentation {
            view,
            action: PresentationAction::Frame { focused: true },
        })
        .unwrap()
    {
        Response::Presentation(frame) => frame,
        _ => panic!("frame expected"),
    }
}
#[test]
fn same_lease_at_coordinates_are_rejected_after_a_queued_scroll_then_fresh_sample_can_apply() {
    let mut host = Harness::start("--child-presentations");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("PRESENTATION_READY"));
    let observer = match host
        .request(Command::Observe {
            viewer: 71,
            after: None,
        })
        .unwrap()
    {
        Response::Observed(observer) => observer,
        _ => panic!("observer expected"),
    };
    let view = PresentationLease::Observer(observer);
    host.request(Command::Presentation {
        view,
        action: PresentationAction::Selection {
            selection: Some(Selection::Word(ViewportPoint { x: 1, y: 0 })),
        },
    })
    .unwrap();
    let sampled = frame(&mut host, view);
    let existing = sampled.selection.unwrap();
    let scroll = host.send(Command::Presentation {
        view,
        action: PresentationAction::Scroll { delta: -1 },
    });
    let guarded = host.send(Command::Presentation {
        view,
        action: PresentationAction::SelectionAtVersion {
            version: sampled.version,
            selection: Some(Selection::Range {
                start: ViewportPoint { x: 6, y: 1 },
                end: ViewportPoint { x: 1, y: 0 },
                rectangle: true,
            }),
        },
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut scrolled, mut rejected) = (false, false);
    while !scrolled || !rejected {
        assert!(
            Instant::now() < deadline,
            "queued selection fence timed out"
        );
        match host.runtime.try_recv() {
            Ok(Event::Reply { id, result }) if id == scroll => {
                assert!(matches!(result, Ok(Response::Applied)));
                scrolled = true;
            }
            Ok(Event::Reply { id, result }) if id == guarded => {
                assert!(matches!(
                    result,
                    Err(RuntimeError {
                        code: ErrorCode::StalePresentation,
                        ..
                    })
                ));
                rejected = true;
            }
            Ok(event) => host.observe(event),
            Err(_) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    let fresh = frame(&mut host, view);
    assert_eq!(fresh.view, sampled.view);
    assert!(fresh.version > sampled.version);
    assert_ne!(fresh.top_row, sampled.top_row);
    let preserved = fresh.selection.unwrap();
    assert_eq!(
        (preserved.anchor, preserved.focus, preserved.kind),
        (existing.anchor, existing.focus, existing.kind)
    );
    host.request(Command::Presentation {
        view,
        action: PresentationAction::SelectionAtVersion {
            version: fresh.version,
            selection: Some(Selection::Range {
                start: ViewportPoint { x: 6, y: 1 },
                end: ViewportPoint { x: 1, y: 0 },
                rectangle: true,
            }),
        },
    })
    .unwrap();
    let applied = frame(&mut host, view).selection.unwrap();
    assert_eq!(applied.kind, PresentationSelectionKind::Range);
    assert!(applied.rectangle);
    assert_eq!(
        applied.anchor,
        HistoryPoint {
            x: 6,
            row: fresh.top_row + 1
        }
    );
    assert_eq!(
        applied.focus,
        HistoryPoint {
            x: 1,
            row: fresh.top_row
        }
    );
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|host| host.exited);
}
#[test]
fn queued_source_resize_rejects_old_sample_even_before_a_new_observer_frame_is_requested() {
    let mut host = Harness::start("--child-presentations");
    host.until(|host| String::from_utf8_lossy(&host.output).contains("PRESENTATION_READY"));
    let observer = match host
        .request(Command::Observe {
            viewer: 72,
            after: None,
        })
        .unwrap()
    {
        Response::Observed(observer) => observer,
        _ => panic!("observer expected"),
    };
    let view = PresentationLease::Observer(observer);
    let sampled = frame(&mut host, view);
    host.request(Command::Resize {
        lease: host.lease,
        cols: 60,
        rows: 20,
    })
    .unwrap();
    let error = host
        .request(Command::Presentation {
            view,
            action: PresentationAction::SelectionAtVersion {
                version: sampled.version,
                selection: Some(Selection::All),
            },
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StalePresentation);
    let fresh = frame(&mut host, view);
    assert!(fresh.selection.is_none());
    assert_eq!(
        (fresh.screen.screen.cols, fresh.screen.screen.rows),
        (60, 20)
    );
    assert_eq!(host.runtime.phase(), Phase::Active);
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|host| host.exited);
}
