use crate::activity::*;
use shellfox_core::{default_process_rules, Generation, SessionId, TabId};
use shellfox_runtime::Identity;
use shellfox_terminal::wsl::{GuestProcess, GuestProcesses};
use std::sync::Arc;

const WSL: &str = r"C:\Windows\System32\wsl.exe";

/// A WSL tab: its Windows root is wsl.exe; agents come only from `guest`.
struct Wsl {
    root: RootState,
    guest: Option<GuestEvidence>,
}
impl ProcessInspector for Wsl {
    fn inspect(&mut self, identity: Identity) -> Result<OwnedProcessSnapshot, InspectionFailure> {
        Ok(OwnedProcessSnapshot {
            identity,
            platform: ProcessPlatform::Windows,
            root: self.root,
            complete: true,
            processes: vec![OwnedProcess {
                identity: ProcessIdentity {
                    domain: "windows/local".into(),
                    pid: 10,
                    birth: "10".into(),
                },
                is_root: true,
                accessible: true,
                executable: Some(WSL.into()),
                process_name: None,
                argv: None,
            }],
        })
    }
    fn guest(&mut self, _: Identity) -> Option<GuestEvidence> {
        self.guest.take()
    }
}
fn guest(marker: &str, executable: &str, accessible: bool) -> GuestProcess {
    GuestProcess {
        marker: marker.into(),
        accessible,
        executable: Some(executable.into()),
        process_name: None,
        argv: Some(vec![executable.into()]),
    }
}
fn listed(tab: TabId, processes: Vec<GuestProcess>, complete: bool) -> GuestEvidence {
    GuestEvidence::Listed {
        listing: Arc::new(GuestProcesses {
            processes,
            complete,
        }),
        marker: tab.to_string(),
    }
}
fn observe(root: RootState, guest: impl FnOnce(TabId) -> GuestEvidence) -> Observation {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let mut inspector = Wsl {
        root,
        guest: Some(guest(identity.tab_id)),
    };
    let mut monitor = ActivityMonitor::new();
    monitor.watch(identity, WSL.into()).unwrap();
    monitor
        .poll(&mut inspector, &default_process_rules(), 0, 1)
        .remove(0)
        .observation
}

#[test]
fn marked_guest_agents_count_for_their_tab_only() {
    let seen = observe(RootState::Alive, |tab| {
        let marker = tab.to_string();
        listed(
            tab,
            vec![
                guest(&marker, "/usr/bin/bash", true),
                guest(&marker, "/home/me/.local/bin/claude", true),
                guest(&TabId::new().to_string(), "/usr/bin/codex", true),
            ],
            true,
        )
    });
    assert_eq!(
        (seen.root, seen.health, seen.agents),
        (RootState::Alive, Health::Healthy, 1)
    );
    assert_eq!(
        tab_status(TabLifecycle::Open, false, Some(&seen)),
        TabStatus::Running
    );
}

#[test]
fn a_guest_without_agents_is_waiting() {
    let seen = observe(RootState::Alive, |tab| {
        let marker = tab.to_string();
        listed(tab, vec![guest(&marker, "/usr/bin/bash", true)], true)
    });
    assert_eq!((seen.health, seen.agents), (Health::Healthy, 0));
    assert_eq!(
        tab_status(TabLifecycle::Open, false, Some(&seen)),
        TabStatus::Waiting
    );
}

#[test]
fn missing_partial_or_unreadable_guest_evidence_is_never_idle() {
    let cases: [Box<dyn FnOnce(TabId) -> GuestEvidence>; 3] = [
        Box::new(|_| GuestEvidence::Unavailable),
        Box::new(|tab| listed(tab, Vec::new(), false)),
        Box::new(|tab| {
            let marker = tab.to_string();
            listed(tab, vec![guest(&marker, "/usr/bin/claude", false)], true)
        }),
    ];
    for case in cases {
        let seen = observe(RootState::Alive, case);
        assert_eq!((seen.health, seen.agents), (Health::Unknown, 0));
        assert_eq!(
            tab_status(TabLifecycle::Open, false, Some(&seen)),
            TabStatus::Unknown
        );
    }
    // A found agent still shows, as in 0.9, while health stays unknown.
    let seen = observe(RootState::Alive, |tab| {
        let marker = tab.to_string();
        listed(tab, vec![guest(&marker, "/usr/bin/claude", true)], false)
    });
    assert_eq!((seen.health, seen.agents), (Health::Unknown, 1));
}

#[test]
fn guest_evidence_never_revives_an_exited_root() {
    let seen = observe(RootState::Exited, |tab| {
        let marker = tab.to_string();
        listed(tab, vec![guest(&marker, "/usr/bin/claude", true)], true)
    });
    assert_eq!((seen.root, seen.agents), (RootState::Exited, 0));
}
