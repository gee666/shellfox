use super::*;
use crate::activity::integration::ActivityUtcAnchor;
use crate::activity::{Health, Observation};
use crate::tests::{default_catalog_profile, scratch};
use shellfox_core::{CreateSession, SettingsPatch};
use std::time::Duration;

const HOUR: u64 = 3_600_000;
const T0: u64 = 1_767_225_600_000; // 2026-01-01T00:00:00Z

fn at(ms: u64) -> String {
    shellfox_host::activity_utc_timestamp(ms).unwrap()
}
fn session(updated: u64, settled: Option<u64>, pinned: bool) -> Session {
    Session {
        id: SessionId::new(),
        title: "s".into(),
        cwd: "/work".into(),
        created_at: at(T0),
        updated_at: at(updated),
        settled_at: settled.map(at),
        pinned_at: pinned.then(|| at(T0)),
        env: Vec::new(),
        tabs: Vec::new(),
    }
}
fn hours(settle: f64, delete: f64) -> Settings {
    Settings {
        session_settle_hours: settle,
        session_delete_hours: delete,
        ..Settings::default()
    }
}
fn persistence() -> ActivityPersistence {
    ActivityPersistence::new(ActivityUtcAnchor {
        logical_ms: T0,
        unix_ms: T0,
    })
}

#[test]
fn settle_and_delete_times_follow_0_9() {
    let p = persistence();
    let s = hours(2.0, 3.0);
    let idle = session(T0, None, false);
    assert_eq!(
        next_due(std::slice::from_ref(&idle), &s, &p),
        Some(T0 + 2 * HOUR)
    );
    // Deletion counts from the later of settling and the last action.
    let settled = session(T0 + HOUR, Some(T0), false);
    assert_eq!(
        next_due(std::slice::from_ref(&settled), &s, &p),
        Some(T0 + 4 * HOUR)
    );
    let resettled = session(T0, Some(T0 + HOUR), false);
    assert_eq!(next_due(&[resettled], &s, &p), Some(T0 + 4 * HOUR));
    // Fractional hours, as the settings allow.
    assert_eq!(
        next_due(std::slice::from_ref(&idle), &hours(0.5, 1.0), &p),
        Some(T0 + HOUR / 2)
    );
    assert_eq!(
        next_due(&[idle.clone(), settled.clone()], &s, &p),
        Some(T0 + 2 * HOUR)
    );
    assert_eq!(
        expiries(&[idle.clone(), settled.clone()], &s, &p, T0 + 2 * HOUR),
        vec![(idle.id, ExpiryAction::Settle)]
    );
    assert_eq!(
        expiries(&[idle.clone(), settled.clone()], &s, &p, T0 + 4 * HOUR),
        vec![
            (idle.id, ExpiryAction::Settle),
            (settled.id, ExpiryAction::Delete)
        ]
    );
    assert!(expiries(&[idle], &s, &p, T0 + 2 * HOUR - 1).is_empty());
}

#[test]
fn pinned_sessions_never_expire() {
    let p = persistence();
    let s = hours(1.0, 1.0);
    let pinned = [session(T0, None, true), session(T0, Some(T0), true)];
    assert_eq!(next_due(&pinned, &s, &p), None);
    assert!(expiries(&pinned, &s, &p, u64::MAX / 2).is_empty());
}

#[test]
fn unwritten_touches_postpone_the_deadline() {
    let mut p = persistence();
    let s = hours(1.0, 1.0);
    let idle = session(T0, None, false);
    p.touch(idle.id, T0 + 30 * 60_000).unwrap();
    assert_eq!(
        next_due(std::slice::from_ref(&idle), &s, &p),
        Some(T0 + 90 * 60_000)
    );
    assert!(expiries(&[idle], &s, &p, T0 + HOUR).is_empty());
}

/// Hours are validated to 1..=87600 on every write, so 0 cannot reach the
/// sweep. A corrupt value never expires anything.
#[test]
fn invalid_hours_never_expire() {
    let p = persistence();
    let idle = [session(T0, None, false)];
    assert_eq!(next_due(&idle, &hours(f64::NAN, 1.0), &p), None);
    assert_eq!(next_due(&idle, &hours(-1.0, 1.0), &p), None);
    assert!(hours(0.0, 1.0).validate().is_err());
    assert!(hours(1.0, 0.0).validate().is_err());
}

struct Fixture {
    _data: tempfile::TempDir,
    service: Service,
    session: SessionId,
    now: u64,
}
impl Fixture {
    fn new() -> Self {
        Self::with_supervisor(&std::env::current_exe().unwrap())
    }
    fn with_supervisor(supervisor: &std::path::Path) -> Self {
        let data = scratch();
        let service = Service::open_owned(data.path(), supervisor).unwrap();
        service
            .host()
            .save_global_settings(SettingsPatch {
                session_settle_hours: Some(1.0),
                session_delete_hours: Some(1.0),
                ..SettingsPatch::default()
            })
            .unwrap();
        let created = service
            .host()
            .create_session(CreateSession {
                request_id: SessionId::new(),
                title: None,
                cwd: data.path().to_string_lossy().into_owned(),
                profile_id: default_catalog_profile(),
            })
            .unwrap();
        let now = service.activity.now_ms().unwrap();
        Self {
            session: created.sessions[0].id,
            _data: data,
            service,
            now,
        }
    }
    fn current(&self) -> Option<Session> {
        let snapshot = self.service.host().snapshot().unwrap();
        snapshot.sessions.into_iter().find(|s| s.id == self.session)
    }
    fn observations(&self) -> Vec<Option<Observation>> {
        let service = &self.service;
        service
            .terminals
            .values()
            .map(|terminal| {
                let runtime = &terminal.runtime;
                service
                    .activity
                    .owner
                    .terminal_facts(runtime.identity(), runtime.phase())
                    .observation
            })
            .collect()
    }
    /// Waits in real time until every shell has started and its processes
    /// have stopped changing for a second. Startup commands, such as a login
    /// shell's profile scripts, are real activity and would otherwise keep
    /// postponing expiry while the test's clock jumps hours ahead.
    fn wait_until_idle(&mut self) {
        let started = Instant::now();
        let mut quiet = Instant::now();
        loop {
            self.service.pump();
            let now = self.service.activity.now_ms().unwrap();
            let service = &mut self.service;
            let updates = service.activity.owner.poll_processes(
                &service.terminals,
                &service.activity.rules,
                now,
                16,
                true,
            );
            if updates.iter().any(|update| update.activity_changed) {
                quiet = Instant::now();
            }
            let observations = self.observations();
            let healthy = observations.iter().all(|observation| {
                observation
                    .as_ref()
                    .is_some_and(|observation| observation.health == Health::Healthy)
            });
            if healthy && quiet.elapsed() >= Duration::from_secs(1) {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the shell never went idle: {observations:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    /// Runs the owner at its own retention deadlines, as `owner_pass_timeout`
    /// would, until `done` holds.
    fn run_until(&mut self, limit: u64, done: impl Fn(&Self) -> bool) {
        let started = Instant::now();
        while !done(self) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "retention stalled"
            );
            let next = self.service.retention.next_ms().unwrap_or(self.now);
            self.now = next.max(self.now + REFRESH_STEP_MS);
            assert!(
                self.now <= limit,
                "nothing happened before {limit}: {:?}",
                self.observations()
            );
            self.service.pump();
            self.service.pump_retention_at(self.now);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn an_idle_session_settles_then_is_deleted() {
    let mut f = Fixture::new();
    let start = f.now;
    f.service.pump_retention_at(start);
    // The first sweep waits one interval after start, then the settle time.
    let due = f.service.retention.next_ms().unwrap();
    assert!(due >= start + HOUR - 1000 && due <= start + HOUR + 1000);
    f.service.pump_retention_at(due - 1);
    assert!(f.current().unwrap().settled_at.is_none());
    f.run_until(start + 2 * HOUR, |f| {
        f.current().unwrap().settled_at.is_some()
    });
    // The settled marker is wall time, so the next deadline is an hour from now.
    let settled = f
        .service
        .activity
        .persistence
        .logical_ms(f.current().unwrap().settled_at.as_deref().unwrap());
    assert_eq!(
        f.service.retention.next_ms(),
        settled.map(|settled| settled.max(start) + HOUR)
    );
    f.run_until(start + 4 * HOUR, |f| f.current().is_none());
    assert!(f.service.retention.next_ms().is_none());
    f.service.shutdown().unwrap();
}

#[test]
fn a_pinned_session_schedules_nothing() {
    let mut f = Fixture::new();
    f.service
        .host()
        .set_session_pinned(f.session, true)
        .unwrap();
    f.service.pump_retention_at(f.now + 1000 * HOUR);
    assert!(f.service.retention.next_ms().is_none());
    assert!(f.service.retention_deadline().is_none());
    assert!(f.current().unwrap().settled_at.is_none());
    f.service.shutdown().unwrap();
}

#[test]
fn the_owner_sleeps_until_the_first_sweep() {
    let mut f = Fixture::new();
    f.service.pump_retention_at(f.now);
    let deadline = f.service.retention_deadline().unwrap();
    let left = deadline.saturating_duration_since(Instant::now());
    assert!(left > Duration::from_secs(3590) && left <= Duration::from_secs(3600));
    f.service.shutdown().unwrap();
}

/// A live shell settles without being closed, then deletion closes it.
#[cfg(any(windows, target_os = "linux"))]
#[test]
fn deleting_closes_live_terminals() {
    // Cargo builds the package's binaries next to the test's deps directory.
    let supervisor = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(format!(
            "shellfox-test-supervisor{}",
            std::env::consts::EXE_SUFFIX
        ));
    assert!(
        supervisor.is_file(),
        "build it first: cargo build -p shellfox-service --bin shellfox-test-supervisor"
    );
    let mut f = Fixture::with_supervisor(&supervisor);
    let tab = f.current().unwrap().tabs[0].id;
    f.service.attach(tab, 1, false).unwrap();
    assert_eq!(f.service.active_terminals(), 1);
    f.wait_until_idle();
    let start = f.now;
    f.run_until(start + 4 * HOUR, |f| {
        f.current().unwrap().settled_at.is_some()
    });
    assert_eq!(f.service.active_terminals(), 1, "settling keeps the shell");
    f.run_until(start + 8 * HOUR, |f| f.current().is_none());
    assert_eq!(f.service.active_terminals(), 0);
    f.service.shutdown().unwrap();
}

#[test]
fn selecting_a_session_restarts_its_clock() {
    let mut f = Fixture::new();
    f.service.pump_retention_at(f.now);
    let due = f.service.retention.next_ms().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    f.service
        .application_metadata(shellfox_host::HostRequest::ActivateSession {
            session_id: f.session,
        })
        .unwrap();
    assert!(f.service.activity.persistence.pending(f.session).is_some());
    f.service.pump_retention_at(due);
    assert!(f.current().unwrap().settled_at.is_none());
    assert!(f.service.retention.next_ms().unwrap() > due);
    f.service.shutdown().unwrap();
}
