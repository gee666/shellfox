use shellfox_core::{default_process_rules, Generation, SessionId, TabId};
use shellfox_runtime::Identity;
use shellfox_service::activity::*;

fn id() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn process(pid: u32, root: bool) -> OwnedProcess {
    let executable = if root { "/bin/bash" } else { "/usr/bin/node" };
    OwnedProcess {
        identity: ProcessIdentity {
            domain: "host/local".into(),
            pid,
            birth: format!("boot:{pid}"),
        },
        is_root: root,
        accessible: true,
        executable: Some(executable.into()),
        process_name: None,
        argv: Some(vec![
            executable.into(),
            "/pkg/@earendil-works/pi-coding-agent/dist/cli.js".into(),
        ]),
    }
}
fn snapshot(identity: Identity) -> OwnedProcessSnapshot {
    OwnedProcessSnapshot {
        identity,
        platform: ProcessPlatform::Unix,
        root: RootState::Alive,
        complete: true,
        processes: vec![process(10, true), process(11, false)],
    }
}
struct Fake<F> {
    calls: Vec<Identity>,
    build: F,
}
impl<F: FnMut(Identity, usize) -> Result<OwnedProcessSnapshot, InspectionFailure>> ProcessInspector
    for Fake<F>
{
    fn inspect(&mut self, identity: Identity) -> Result<OwnedProcessSnapshot, InspectionFailure> {
        self.calls.push(identity);
        (self.build)(identity, self.calls.len())
    }
}

#[test]
fn no_watch_no_process_work_and_watch_polling_is_bounded_and_fair() {
    let mut fake = Fake {
        calls: Vec::new(),
        build: |identity, _| Ok(snapshot(identity)),
    };
    let mut monitor = ActivityMonitor::new();
    assert!(monitor.poll(&mut fake, &[], 0, 16).is_empty());
    assert!(fake.calls.is_empty());
    let ids: Vec<_> = (0..20).map(|_| id()).collect();
    for identity in &ids {
        monitor.watch(*identity, "/bin/bash".into()).unwrap();
    }
    assert_eq!(monitor.poll(&mut fake, &[], 0, usize::MAX).len(), 16);
    assert_eq!(monitor.poll(&mut fake, &[], 0, 16).len(), 4);
    assert!(monitor.poll(&mut fake, &[], 999, 16).is_empty());
    assert_eq!(fake.calls, ids);
    assert_eq!(monitor.poll(&mut fake, &[], 1000, 1).len(), 1);
}

#[test]
fn names_are_not_ownership_and_root_birth_is_pinned_to_generation() {
    let identity = id();
    let mut fake = Fake {
        calls: Vec::new(),
        build: |identity, call| {
            let mut data = snapshot(identity);
            if call == 2 {
                data.processes[0].identity.birth = "reused-root".into();
            }
            if call == 3 {
                data.identity.generation = Generation::new();
            }
            Ok(data)
        },
    };
    let mut monitor = ActivityMonitor::new();
    monitor.watch(identity, "/bin/bash".into()).unwrap();
    let rules = default_process_rules();
    let first = monitor.poll(&mut fake, &rules, 0, 1).remove(0);
    assert_eq!(
        (
            first.observation.root,
            first.observation.health,
            first.observation.agents
        ),
        (RootState::Alive, Health::Healthy, 1)
    );
    let reused = monitor.poll(&mut fake, &rules, 1000, 1).remove(0);
    assert_eq!(
        (reused.observation.root, reused.observation.agents),
        (RootState::Unavailable, 0)
    );
    let stale = monitor.poll(&mut fake, &rules, 2000, 1).remove(0);
    assert_eq!(stale.observation.root, RootState::Unavailable);
    assert!(stale.observation.reason.unwrap().contains("generation"));
}

#[test]
fn root_exec_access_and_partial_enumeration_cannot_fake_healthy_waiting() {
    for condition in 0..4 {
        let identity = id();
        let mut fake = Fake {
            calls: Vec::new(),
            build: move |identity, _| {
                let mut data = snapshot(identity);
                match condition {
                    0 => data.processes[0].executable = Some("/usr/bin/node".into()),
                    1 => data.processes[0].accessible = false,
                    2 => data.complete = false,
                    _ => data.processes[1].argv = None,
                }
                Ok(data)
            },
        };
        let mut monitor = ActivityMonitor::new();
        monitor.watch(identity, "/bin/bash".into()).unwrap();
        let update = monitor
            .poll(&mut fake, &default_process_rules(), 0, 1)
            .remove(0);
        assert_eq!(update.observation.health, Health::Unknown);
        assert!(update.observation.reason.is_some());
    }
}

#[test]
fn exact_descendant_replacement_is_activity_but_poll_time_is_not() {
    let identity = id();
    let mut fake = Fake {
        calls: Vec::new(),
        build: |identity, call| {
            let mut data = snapshot(identity);
            if call >= 3 {
                data.processes[1].identity.birth = "replacement-birth".into();
            }
            Ok(data)
        },
    };
    let mut monitor = ActivityMonitor::new();
    monitor.watch(identity, "/bin/bash".into()).unwrap();
    let rules = default_process_rules();
    let first = monitor.poll(&mut fake, &rules, 0, 1).remove(0);
    assert!(first.activity_changed && first.touch_session);
    let stable = monitor.poll(&mut fake, &rules, 1000, 1).remove(0);
    assert!(!stable.activity_changed && !stable.touch_session);
    let changed = monitor.poll(&mut fake, &rules, 2000, 1).remove(0);
    assert!(
        changed.activity_changed && changed.touch_session,
        "real descendant birth changes preserve live TTL policy"
    );
    let stable = monitor.poll(&mut fake, &rules, 3000, 1).remove(0);
    assert!(!stable.activity_changed && !stable.touch_session);
    monitor.rules_changed();
    assert!(monitor.observation(identity).is_none());
    assert!(!monitor.poll(&mut fake, &rules, 3000, 1)[0].activity_changed);
}

#[test]
fn hidden_watches_pause_but_explicit_expiry_refresh_checks_owned_generations() {
    let first = id();
    let second = id();
    let mut monitor = ActivityMonitor::new();
    monitor.watch(first, "/bin/bash".into()).unwrap();
    monitor.watch(second, "/bin/bash".into()).unwrap();
    monitor.set_visible_sessions(Some(&[])).unwrap();
    let mut fake = Fake {
        calls: Vec::new(),
        build: |identity, _| Ok(snapshot(identity)),
    };
    assert!(monitor.poll(&mut fake, &[], 0, 16).is_empty());
    assert!(fake.calls.is_empty());
    monitor
        .set_visible_sessions(Some(&[first.session_id]))
        .unwrap();
    assert_eq!(monitor.poll(&mut fake, &[], 0, 16)[0].identity, first);
    let all = monitor.poll_all(&mut fake, &[], 0, 16);
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|update| update.identity == second));
    assert!(monitor.fresh_healthy(&[first, second], 0));
    assert!(!monitor.fresh_healthy(&[first, second], 1));
    assert!(!monitor.fresh_healthy(
        &[Identity {
            generation: Generation::new(),
            ..first
        }],
        0
    ));
    monitor.forget(Identity {
        generation: Generation::new(),
        ..first
    });
    assert!(monitor.observation(first).is_some());
    monitor.forget(first);
    assert!(monitor.observation(first).is_none());
    assert!(!monitor.fresh_healthy(&[first], 0));
}

#[test]
fn malformed_owned_snapshots_fail_closed_with_sanitized_static_reasons() {
    for condition in 0..9 {
        let identity = id();
        let mut fake = Fake {
            calls: Vec::new(),
            build: move |identity, _| {
                let mut data = snapshot(identity);
                match condition {
                    0 => data.processes.push(process(11, false)),
                    1 => data.processes[1].identity.birth.clear(),
                    2 => data.processes[1].identity.pid = 0,
                    3 => data.processes[1].argv = Some(vec!["SECRET".repeat(20000)]),
                    4 => data.processes[1].is_root = true,
                    5 => data.processes[1].identity.domain = "unowned/foreign".into(),
                    6 => data.processes[1].identity.birth = "x".repeat(257),
                    7 => data.processes[1].argv = Some(vec!["x".repeat(64 * 1024); 129]),
                    _ => return Err(InspectionFailure::Unavailable),
                }
                Ok(data)
            },
        };
        let mut monitor = ActivityMonitor::new();
        monitor.watch(identity, "/bin/bash".into()).unwrap();
        let result = monitor.poll(&mut fake, &[], 0, 1).remove(0);
        assert_eq!(result.observation.root, RootState::Unavailable);
        assert_eq!(result.observation.agents, 0);
        assert!(!format!("{result:?}").contains("SECRET"));
    }
}

#[test]
fn duplicate_rules_do_not_double_count_and_root_is_never_an_agent() {
    let identity = id();
    let mut rules = default_process_rules();
    rules.push(rules[0].clone());
    let mut fake = Fake {
        calls: Vec::new(),
        build: |identity, _| Ok(snapshot(identity)),
    };
    let mut monitor = ActivityMonitor::new();
    monitor.watch(identity, "/bin/bash".into()).unwrap();
    assert_eq!(
        monitor.poll(&mut fake, &rules, 0, 1)[0].observation.agents,
        1
    );
    let mut shell_rule = rules[0].clone();
    shell_rule.executable_basenames = vec!["bash".into()];
    shell_rule.script_path_suffixes.clear();
    shell_rule.process_names = None;
    assert_eq!(
        monitor.poll_all(&mut fake, &[shell_rule], 0, 1)[0]
            .observation
            .agents,
        0
    );
}

#[test]
fn large_owned_jobs_share_a_bounded_identity_cache_and_release_it_on_forget() {
    let first = id();
    let second = id();
    let mut monitor = ActivityMonitor::new();
    monitor.watch(first, "/bin/bash".into()).unwrap();
    monitor.watch(second, "/bin/bash".into()).unwrap();
    let mut fake = Fake {
        calls: Vec::new(),
        build: |identity, _| {
            let mut data = snapshot(identity);
            data.processes = vec![process(10, true)];
            data.processes
                .extend((11..10_012).map(|pid| process(pid, false)));
            Ok(data)
        },
    };
    let updates = monitor.poll(&mut fake, &[], 0, 16);
    assert_eq!(updates[0].observation.health, Health::Healthy);
    assert_eq!(updates[1].observation.health, Health::Unknown);
    assert!(!monitor.fresh_healthy(&[first, second], 0));
    monitor.forget(first);
    let recovered = monitor.poll_all(&mut fake, &[], 1, 1);
    assert_eq!(recovered[0].identity, second);
    assert_eq!(recovered[0].observation.health, Health::Healthy);
}

#[test]
fn invalid_rules_and_watch_limit_fail_without_spawning_or_reading_other_processes() {
    let mut monitor = ActivityMonitor::new();
    assert!(monitor.watch(id(), String::new()).is_err());
    for _ in 0..MAX_WATCHES {
        monitor.watch(id(), "/bin/bash".into()).unwrap();
    }
    assert!(monitor.watch(id(), "/bin/bash".into()).is_err());
    let mut fake = Fake {
        calls: Vec::new(),
        build: |identity, _| Ok(snapshot(identity)),
    };
    let mut rules = default_process_rules();
    rules[0].id = "invalid".into();
    assert_eq!(
        monitor.poll(&mut fake, &rules, 0, 1)[0].observation.root,
        RootState::Unavailable
    );
    assert!(fake.calls.is_empty());
}
