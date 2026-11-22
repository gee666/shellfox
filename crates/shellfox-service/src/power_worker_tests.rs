//! Mock-backend worker regressions.
use super::*;
use super::super::{worker::PowerWorker, PowerReport, PowerRequest, MAX_POWER_MESSAGE_BYTES};
use shellfox_protocol::{CancellationToken, Validate};
use std::time::{Duration, Instant};

fn start(worker: &mut PowerWorker<Backend>, active: bool) {
    worker
        .start(active, CancellationToken::new(), CancellationToken::new())
        .unwrap();
}
fn completed(worker: &mut PowerWorker<Backend>) -> PowerReport {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        worker.poll();
        let report = worker.report();
        if !report.pending {
            return report;
        }
        assert!(Instant::now() < deadline, "mock worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn worker_starts_off_and_preserves_partial_and_release_failure() {
    let state = Arc::new(Mutex::new(State {
        partial: true,
        ..State::default()
    }));
    let mut worker = PowerWorker::new(Backend(state.clone()));
    assert_eq!(worker.report().status, PowerStatus::Inactive);
    assert_eq!(state.lock().unwrap().acquires, 0);
    start(&mut worker, true);
    assert!(worker.report().pending);
    assert!(worker
        .start(true, CancellationToken::new(), CancellationToken::new())
        .is_err());
    let report = completed(&mut worker);
    assert_eq!(report.status, PowerStatus::Partial);
    assert_eq!(report.partial_reason.as_deref(), Some("lid denied"));
    report.validate().unwrap();
    state.lock().unwrap().fail_release = true;
    start(&mut worker, false);
    let report = completed(&mut worker);
    assert_eq!(report.status, PowerStatus::ReleaseFailed);
    assert_eq!(report.error.as_deref(), Some("release failed"));
    report.validate().unwrap();
    start(&mut worker, true);
    assert_eq!(completed(&mut worker).status, PowerStatus::ReleaseFailed);
    state.lock().unwrap().fail_release = false;
    assert!(worker.stop().is_err());
    assert_eq!(completed(&mut worker).status, PowerStatus::Inactive);
    worker.stop().unwrap();
    let state = state.lock().unwrap();
    assert_eq!((state.acquires, state.drops), (1, 0));
}
#[test]
fn failed_acquire_is_inactive_with_error_and_stop_clears_it() {
    let state = Arc::new(Mutex::new(State {
        fail_acquire: true,
        ..State::default()
    }));
    let mut worker = PowerWorker::new(Backend(state));
    start(&mut worker, true);
    let report = completed(&mut worker);
    assert_eq!(report.status, PowerStatus::Inactive);
    assert_eq!(report.error.as_deref(), Some("acquire failed"));
    assert!(worker.stop().is_err());
    completed(&mut worker);
    worker.stop().unwrap();
}

struct GatedBackend {
    state: Arc<Mutex<State>>,
    entered: std::sync::mpsc::SyncSender<()>,
    resume: std::sync::mpsc::Receiver<()>,
}
impl PowerBackend for GatedBackend {
    type Lease = Lease;
    fn acquire(&mut self) -> Result<Lease, PowerError> {
        self.entered.send(()).unwrap();
        self.resume.recv().unwrap();
        Backend(self.state.clone()).acquire()
    }
}
#[test]
fn in_flight_enable_never_survives_stop_and_owner_poll_does_not_wait() {
    let state = Arc::new(Mutex::new(State::default()));
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (resume, waiting) = std::sync::mpsc::sync_channel(1);
    let mut worker = PowerWorker::new(GatedBackend {
        state: state.clone(),
        entered,
        resume: waiting,
    });
    worker
        .start(true, CancellationToken::new(), CancellationToken::new())
        .unwrap();
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    // Worker holds the manager mutex inside acquire. These owner operations
    // must return without trying to take it.
    worker.poll();
    assert!(worker.report().pending);
    assert!(worker.stop().is_err());
    resume.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker.report().pending {
        worker.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(worker.report().status, PowerStatus::Inactive);
    worker.stop().unwrap();
    assert!(worker
        .start(true, CancellationToken::new(), CancellationToken::new())
        .is_err());
    let state = state.lock().unwrap();
    assert_eq!((state.acquires, state.releases, state.drops), (1, 1, 0));
}
#[test]
fn canceled_enable_does_not_acquire_or_release_an_existing_lease() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut worker = PowerWorker::new(Backend(state.clone()));
    let cancel = CancellationToken::new();
    cancel.cancel();
    worker
        .start(true, cancel.clone(), CancellationToken::new())
        .unwrap();
    assert_eq!(completed(&mut worker).status, PowerStatus::Inactive);
    assert_eq!(state.lock().unwrap().acquires, 0);
    start(&mut worker, true);
    completed(&mut worker);
    worker
        .start(true, cancel, CancellationToken::new())
        .unwrap();
    let report = completed(&mut worker);
    assert_eq!(report.status, PowerStatus::Active);
    assert!(report.error.is_some());
    assert_eq!(state.lock().unwrap().releases, 0);
    start(&mut worker, false);
    completed(&mut worker);
}
#[test]
fn shutdown_failure_keeps_same_lease_until_successful_retry() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut worker = PowerWorker::new(Backend(state.clone()));
    start(&mut worker, true);
    completed(&mut worker);
    state.lock().unwrap().fail_release = true;
    assert!(worker.stop().is_err());
    let failed = completed(&mut worker);
    assert_eq!(failed.status, PowerStatus::ReleaseFailed);
    assert!(failed.error.is_some());
    assert_eq!(state.lock().unwrap().drops, 0);
    state.lock().unwrap().fail_release = false;
    assert!(worker.stop().is_err());
    assert_eq!(completed(&mut worker).status, PowerStatus::Inactive);
    worker.stop().unwrap();
    let state = state.lock().unwrap();
    assert_eq!((state.acquires, state.releases, state.drops), (1, 2, 0));
}

struct PanicBackend;
impl PowerBackend for PanicBackend {
    type Lease = Lease;
    fn acquire(&mut self) -> Result<Lease, PowerError> {
        panic!("mock acquire panic")
    }
}
#[test]
fn panicked_worker_never_confirms_shutdown_or_allows_another_enable() {
    let mut worker = PowerWorker::new(PanicBackend);
    worker
        .start(true, CancellationToken::new(), CancellationToken::new())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker.report().error.is_none() {
        worker.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(worker.report().pending);
    assert!(worker.uncertain());
    assert!(worker.stop().is_err());
    assert!(worker
        .start(true, CancellationToken::new(), CancellationToken::new())
        .is_err());
}

#[test]
fn canceled_in_flight_acquire_releases_only_its_new_lease() {
    let state = Arc::new(Mutex::new(State::default()));
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (resume, waiting) = std::sync::mpsc::sync_channel(1);
    let mut worker = PowerWorker::new(GatedBackend {
        state: state.clone(),
        entered,
        resume: waiting,
    });
    let cancel = CancellationToken::new();
    worker
        .start(true, cancel.clone(), CancellationToken::new())
        .unwrap();
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    cancel.cancel();
    resume.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker.report().pending {
        worker.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(worker.report().status, PowerStatus::Inactive);
    let state = state.lock().unwrap();
    assert_eq!((state.acquires, state.releases, state.drops), (1, 1, 0));
}

#[test]
fn owner_receipt_rechecks_loss_after_finished_enable_and_retains_same_lease() {
    let state = Arc::new(Mutex::new(State::default()));
    let lost = CancellationToken::new();
    let mut worker = PowerWorker::new(Backend(state.clone()));
    worker
        .start(true, CancellationToken::new(), lost.clone())
        .unwrap();
    // Finish the worker BEFORE original ownership is lost. The worker's own
    // pre/post acquire checks have already succeeded. No receipt is admitted yet.
    assert_eq!(completed(&mut worker).status, PowerStatus::Active);
    state.lock().unwrap().fail_release = true;
    lost.cancel();
    // This is the same final receipt-admission method called by Service::pump_power.
    assert!(worker.admit_owner_receipt(&lost).is_err());
    assert!(worker.report().pending);
    let report = completed(&mut worker);
    assert_eq!(report.status, PowerStatus::ReleaseFailed);
    assert!(report.error.is_some());
    assert!(worker.start(true, CancellationToken::new(), lost).is_err());
    {
        let mut state = state.lock().unwrap();
        assert_eq!((state.acquires, state.releases, state.drops), (1, 1, 0));
        state.fail_release = false;
    }
    assert!(worker.stop().is_err());
    assert_eq!(completed(&mut worker).status, PowerStatus::Inactive);
    worker.stop().unwrap();
    let state = state.lock().unwrap();
    assert_eq!((state.acquires, state.releases, state.drops), (1, 2, 0));
}

#[test]
fn reports_and_closed_wire_payloads_are_bounded() {
    let json = serde_json::to_string(&PowerRequest::Set { active: true }).unwrap();
    assert_eq!(json, r#"{"operation":"set","active":true}"#);
    assert!(
        serde_json::from_str::<PowerRequest>(r#"{"operation":"set","active":true,"extra":1}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<PowerRequest>(r#"{"operation":"status","active":true}"#).is_err()
    );
    let mut report = PowerReport {
        status: PowerStatus::Partial,
        partial_reason: None,
        error: None,
        pending: false,
    };
    assert!(report.validate().is_err());
    report.partial_reason = Some("x".repeat(MAX_POWER_MESSAGE_BYTES + 1));
    assert!(report.validate().is_err());
    report.partial_reason = Some("restricted".into());
    report.validate().unwrap();
}
