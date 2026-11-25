use super::*;
use shellfox_core::{Generation, SessionId};
use shellfox_runtime::{Identity, Phase, ScreenFrame};

fn frame(tab: TabId, frame_id: u64, sequence: u64) -> NativeFrame {
    let mut terminal = shellfox_ghostty_vt::Terminal::new(4, 2).unwrap();
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: tab,
        generation: Generation::new(),
    };
    let mut view = terminal.new_presentation_view().unwrap();
    let rendered = terminal.view_render(&mut view).unwrap();
    let screen = Arc::new(ScreenFrame {
        identity,
        sequence,
        screen: rendered.screen,
        mouse_tracking: false,
        alternate_scroll: false,
    });
    let presentation = Arc::new(shellfox_runtime::ViewFrame {
        identity,
        sequence,
        version: frame_id,
        view: shellfox_runtime::PresentationLease::Controller(shellfox_runtime::Lease {
            controller: 1,
            epoch: 1,
        }),
        screen: screen.clone(),
        row_metadata: rendered.row_metadata.into(),
        selection: terminal.view_selection(&mut view).unwrap(),
        cursor_visual: terminal.cursor_visual().unwrap(),
        top_row: rendered.top_row,
        total_rows: rendered.total_rows,
    });
    NativeFrame {
        frame_id,
        frame: screen,
        presentation,
        highest_delivered: sequence,
        recovery_barrier: None,
        exited: None,
        phase: Phase::Active,
        error: None,
        replay_lost: false,
        working_directory: None,
    }
}

#[test]
fn native_admission_is_bounded_and_cancellation_returns_capacity() {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut slots: Vec<_> = (0..16)
        .map(|_| Admission::acquire(counter.clone(), MAX_REQUESTS).unwrap())
        .collect();
    assert!(Admission::acquire(counter.clone(), MAX_REQUESTS).is_err());
    slots.pop();
    assert!(Admission::acquire(counter.clone(), MAX_REQUESTS).is_ok());
    drop(slots);
    assert_eq!(counter.load(Ordering::Acquire), 0);
}

#[test]
fn frame_admission_is_fair_and_bounded_while_waiting() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let held = slots.clone().try_acquire_owned().unwrap();
    let mut first = Box::pin(slots.clone().acquire_owned());
    let mut later = Box::pin(slots.clone().acquire_owned());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert!(later.as_mut().poll(&mut cx).is_pending());
    drop(held);
    assert!(later.as_mut().poll(&mut cx).is_pending());
    let Poll::Ready(Ok(permit)) = first.as_mut().poll(&mut cx) else {
        panic!("first waiter must progress")
    };
    drop(permit);
    assert!(later.as_mut().poll(&mut cx).is_ready());
    let counter = Arc::new(AtomicUsize::new(0));
    let waiters: Vec<_> = (0..MAX_FRAME_WAITERS)
        .map(|_| Admission::acquire(counter.clone(), MAX_FRAME_WAITERS).unwrap())
        .collect();
    assert!(Admission::acquire(counter.clone(), MAX_FRAME_WAITERS).is_err());
    drop(waiters);
    assert_eq!(counter.load(Ordering::Acquire), 0);
}

#[test]
fn latest_frame_is_coalesced_without_copying_cells_and_late_frames_are_rejected() {
    let tab = TabId::new();
    let mut frames = frames::Frames::default();
    let first = frame(tab, 1, 0);
    frames.accept(tab, false, first.clone()).unwrap();
    assert!(Arc::ptr_eq(
        &first.frame,
        &frames.latest(tab, false).unwrap().frame
    ));
    let next = frame(tab, 2, 0); // Selection can repaint without new output.
    frames.accept(tab, false, next.clone()).unwrap();
    assert!(frames.accept(tab, false, first).is_err());
    assert!(Arc::ptr_eq(
        &next.frame,
        &frames.latest(tab, false).unwrap().frame
    ));
    assert_eq!(next.frame.screen.cells.len(), 8);
    frames.remove(tab, false);
    assert!(frames.latest(tab, false).is_none());
}

#[test]
fn confirmed_metadata_close_releases_native_cache_slots() {
    let tab = TabId::new();
    let mut frames = frames::Frames::default();
    frames.accept(tab, false, frame(tab, 1, 0)).unwrap();
    frames.retain_snapshot(&shellfox_core::Snapshot {
        revision: 1,
        sessions: vec![],
        preferences: Default::default(),
        selection: Default::default(),
    });
    assert!(frames.latest(tab, false).is_none());
}

#[test]
fn controller_and_observer_views_have_private_latest_slots() {
    let tab = TabId::new();
    let mut frames = frames::Frames::default();
    let control = frame(tab, 1, 0);
    let mut observer = frame(tab, 2, 0);
    Arc::get_mut(&mut observer.presentation).unwrap().view =
        shellfox_runtime::PresentationLease::Observer(shellfox_runtime::ObserverLease {
            viewer: 2,
            epoch: 1,
        });
    frames.accept(tab, false, control.clone()).unwrap();
    frames.accept(tab, true, observer.clone()).unwrap();
    assert!(Arc::ptr_eq(
        &control.frame,
        &frames.latest(tab, false).unwrap().frame
    ));
    assert!(Arc::ptr_eq(
        &observer.frame,
        &frames.latest(tab, true).unwrap().frame
    ));
    frames.clear();
    assert!(frames.latest(tab, false).is_none() && frames.latest(tab, true).is_none());
}

#[test]
fn frame_cache_rejects_invalid_grid_and_more_than_128_views_before_acceptance() {
    let mut frames = frames::Frames::default();
    let tab = TabId::new();
    let mut invalid = frame(tab, 1, 0);
    let mut terminal = shellfox_ghostty_vt::Terminal::new(4, 2).unwrap();
    let mut screen = terminal.render().unwrap();
    screen.cells.pop();
    invalid.frame = Arc::new(ScreenFrame {
        identity: invalid.frame.identity,
        sequence: 0,
        screen,
        mouse_tracking: false,
        alternate_scroll: false,
    });
    assert!(frames.accept(tab, false, invalid).is_err());
    assert!(frames.latest(tab, false).is_none());
    for id in 1..=128 {
        let tab = TabId::new();
        frames.accept(tab, false, frame(tab, id, 0)).unwrap();
    }
    let tab = TabId::new();
    assert!(frames.accept(tab, false, frame(tab, 129, 0)).is_err());
}

#[test]
fn host_control_refusal_reaches_the_view_as_control_required() {
    let crossed = anyhow::Error::new(shellfox_service::local::LocalRequestFailure {
        method: "desktop",
        error: shellfox_protocol::AppError {
            code: shellfox_protocol::ErrorCode::TerminalControlRequired,
            message: "Terminal control belongs to another controller.".into(),
            retryable: false,
        },
    });
    assert!(crate::terminal::control_required(&super::runtime_error(
        crossed
    )));
    let internal = anyhow::Error::new(shellfox_service::local::LocalRequestFailure {
        method: "desktop",
        error: shellfox_protocol::AppError {
            code: shellfox_protocol::ErrorCode::Internal,
            message: "Private host operation failed.".into(),
            retryable: false,
        },
    });
    assert!(!crate::terminal::control_required(&super::runtime_error(
        internal
    )));
}
