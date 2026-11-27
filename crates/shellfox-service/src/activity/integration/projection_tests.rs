use super::*;
use crate::activity::{Health, Observation, RootState};
use shellfox_core::{
    Generation, Preferences, ProfileId, Selection, Session, SessionId, Snapshot, Tab, TabId,
    TabState,
};
use shellfox_core::{SettingsAdapter, SettingsShell};
use shellfox_host::{SessionCreationFactMap, SessionCreationFacts};
use shellfox_protocol as wire;
use shellfox_protocol::CancellationToken;
use shellfox_remote::adapter;
use shellfox_runtime::Identity;
use shellfox_runtime::Phase;
fn source(creation: bool, observation: Option<Observation>) -> SourceFacts {
    let session_id = SessionId::new();
    let tab_id = TabId::new();
    let snapshot = Snapshot {
        revision: 1,
        preferences: Preferences::default(),
        selection: Selection::default(),
        sessions: vec![Session {
            id: session_id,
            title: "fact fixture".into(),
            cwd: "C:\\fixture".into(),
            created_at: "2025-01-01T00:00:00.000Z".into(),
            updated_at: "2025-01-01T00:00:00.000Z".into(),
            settled_at: None,
            pinned_at: None,
            env: vec![],
            tabs: vec![Tab {
                id: tab_id,
                session_id,
                title: "cmd intent".into(),
                use_process_title: false,
                cwd: "C:\\fixture".into(),
                ordinal: 0,
                created_at: Some("2026-01-01T00:00:00.000Z".into()),
                profile_id: ProfileId::new("cmd").unwrap(),
                wsl_launch: None,
                state: TabState::Pending,
            }],
        }],
    };
    let mut facts = SessionCreationFactMap::new();
    if creation {
        facts.insert(
            session_id,
            SessionCreationFacts::new(SettingsAdapter::EmbeddedPty, SettingsShell::Pwsh),
        );
    }
    let settings = shellfox_core::Settings {
        shell_id: SettingsShell::Bash,
        adapter_id: SettingsAdapter::GnomeTerminal,
        ..Default::default()
    };
    SourceFacts {
        sequence: 1,
        snapshot,
        settings,
        creation_facts: facts,
        #[cfg(windows)]
        wsl_profiles: None,
        cancelled: CancellationToken::new(),
        terminals: vec![TerminalSourceFacts {
            identity: Identity {
                session_id,
                tab_id,
                generation: Generation::new(),
            },
            phase: Phase::Active,
            source_kind: Some(shellfox_runtime::SourceKind::OwnedPty),
            automation_exit_code: None,
            startup: StartupEvidence::from_started(&Ok(())),
            observation,
            busy: false,
            activity_available: true,
            title: None,
        }],
    }
}
#[test]
fn creation_fields_use_only_the_exact_persisted_host_record_not_settings_or_first_tab() {
    let source = source(true, None);
    let facts = project_owned_session_facts(&source).unwrap();
    let session = &source.snapshot.sessions[0];
    let dto = adapter::session_dto(session, &facts[&session.id]).unwrap();
    assert!(matches!(dto.adapter_id, wire::AdapterId::EmbeddedPty));
    assert!(matches!(dto.shell_id, wire::ShellId::Pwsh));
    assert_eq!(dto.tabs[0].created_at, "2026-01-01T00:00:00.000Z");
    assert!(matches!(dto.tabs[0].status, wire::TabStatus::Unknown));
    assert_eq!(
        dto.counts.agents, 0,
        "Active phase is not an agent observation"
    );
}
#[test]
fn missing_legacy_creation_or_tab_timestamp_is_unsupported_not_backfilled() {
    let missing = source(false, None);
    assert!(matches!(
        project_owned_session_facts(&missing),
        Err(wire::AppError {
            code: wire::ErrorCode::Unsupported,
            ..
        })
    ));
    let mut missing = source(true, None);
    missing.snapshot.sessions[0].tabs[0].created_at = None;
    assert!(project_owned_session_facts(&missing).is_err());
    let mut unsupported = source(true, None);
    unsupported
        .creation_facts
        .values_mut()
        .next()
        .unwrap()
        .version = 2;
    assert!(project_owned_session_facts(&unsupported).is_err());
}
#[test]
fn targeted_legacy_collection_ignores_unrelated_unknown_birth_but_complete_inventory_is_strict() {
    let mut source = source(true, None);
    let normal = source.snapshot.sessions[0].id;
    let mut raw = source.snapshot.sessions[0].clone();
    raw.id = SessionId::new();
    raw.tabs[0].id = TabId::new();
    raw.tabs[0].session_id = raw.id;
    raw.tabs[0].profile_id = ProfileId::new("automation-pipe").unwrap();
    let raw_id = raw.id;
    source.snapshot.sessions.push(raw);
    let before = source.snapshot.clone();
    let facts = crate::facts::projection::project_session_facts_for(&source, Some(normal)).unwrap();
    assert_eq!(facts.len(), 1);
    assert!(facts.contains_key(&normal));
    assert!(matches!(
        crate::facts::projection::project_session_facts_for(&source, Some(raw_id)),
        Err(wire::AppError {
            code: wire::ErrorCode::Unsupported,
            retryable: false,
            ..
        })
    ));
    assert!(matches!(
        project_owned_session_facts(&source),
        Err(wire::AppError {
            code: wire::ErrorCode::Unsupported,
            retryable: false,
            ..
        })
    ));
    assert_eq!(
        source.snapshot, before,
        "target scope never filters or mutates the complete snapshot"
    );
}
#[test]
fn legacy_projects_completed_automation_without_inventing_persisted_shell_facts() {
    let mut source = source(false, None);
    let session = &mut source.snapshot.sessions[0];
    session.tabs[0].profile_id = ProfileId::new("automation-pipe").unwrap();
    let facts = &mut source.terminals[0];
    facts.source_kind = Some(shellfox_runtime::SourceKind::ExternalBytes);
    facts.automation_exit_code = Some(7);
    let generation = facts.identity.generation;
    let session = &source.snapshot.sessions[0];
    let projected = project_owned_session_facts(&source).unwrap();
    let dto = adapter::session_dto(session, &projected[&session.id]).unwrap();
    assert!(matches!(dto.adapter_id, wire::AdapterId::EmbeddedPty));
    assert!(matches!(dto.shell_id, wire::ShellId::LoginShell));
    assert!(!dto.can_add_tab);
    assert_eq!(dto.counts.closed, 1);
    let tab = &dto.tabs[0];
    assert!(matches!(tab.lifecycle, wire::TabLifecycle::Closed));
    assert_eq!(tab.exit_code, Some(Some(7)));
    assert_eq!(tab.generation, Some(generation));
    assert!(source.creation_facts.is_empty());
    source.terminals[0].automation_exit_code = None;
    assert!(project_owned_session_facts(&source).is_err());
}

#[test]
fn only_actual_observation_counts_agents_and_sink_failure_is_reported_unknown() {
    let observation = Observation {
        root: RootState::Alive,
        health: Health::Healthy,
        agents: 2,
        reason: None,
        observed_at_ms: 42,
    };
    let mut source = source(true, Some(observation));
    let session = &source.snapshot.sessions[0];
    let facts = project_owned_session_facts(&source).unwrap();
    let dto = adapter::session_dto(session, &facts[&session.id]).unwrap();
    assert!(matches!(dto.activity_status, wire::TabStatus::Running));
    assert_eq!(dto.counts.agents, 2);
    source.terminals[0].activity_available = false;
    let facts = project_owned_session_facts(&source).unwrap();
    let adapter::ObservationFacts::Alive {
        healthy, reason, ..
    } = &facts[&session.id].tabs[&session.tabs[0].id].observation
    else {
        panic!("lost known agent evidence");
    };
    assert!(!healthy);
    assert!(reason.as_deref().unwrap().contains("unavailable"));
}
#[test]
fn draining_with_root_exit_closes_lifecycle_but_retains_cleanup_reason() {
    let mut source = source(
        true,
        Some(Observation {
            root: RootState::Exited,
            health: Health::Unknown,
            agents: 0,
            reason: Some("root exited".into()),
            observed_at_ms: 42,
        }),
    );
    source.terminals[0].phase = Phase::Draining;
    let facts = project_owned_session_facts(&source).unwrap();
    let session = &source.snapshot.sessions[0];
    let dto = adapter::session_dto(session, &facts[&session.id]).unwrap();
    assert!(matches!(dto.tabs[0].lifecycle, wire::TabLifecycle::Closed));
    assert!(dto.tabs[0]
        .monitoring_reason
        .as_deref()
        .unwrap()
        .contains("cleanup"));
    assert_eq!(
        source.terminals[0].phase,
        Phase::Draining,
        "mapping grants no cleanup proof"
    );
}
#[test]
fn draining_failed_start_is_launch_uncertain_and_unknown_start_is_unavailable() {
    let mut source = source(true, None);
    source.terminals[0].phase = Phase::Draining;
    source.terminals[0].startup =
        StartupEvidence::from_started(&Err(shellfox_runtime::RuntimeError {
            code: shellfox_runtime::ErrorCode::Unavailable,
            message: "untrusted native argv secret".into(),
        }));
    let facts = project_owned_session_facts(&source).unwrap();
    let session = &source.snapshot.sessions[0];
    let dto = adapter::session_dto(session, &facts[&session.id]).unwrap();
    assert!(matches!(
        dto.tabs[0].lifecycle,
        wire::TabLifecycle::LaunchUncertain
    ));
    assert!(matches!(
        dto.tabs[0].error.as_ref().unwrap().code,
        wire::ErrorCode::LaunchFailed
    ));
    assert!(!dto.tabs[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("secret"));
    source.terminals[0].startup = StartupEvidence::Pending;
    assert!(project_owned_session_facts(&source).is_err());
}
#[test]
fn closed_is_confirmed_but_draining_never_claims_completed_shutdown() {
    let mut source = source(true, None);
    source.terminals[0].phase = Phase::Draining;
    let facts = project_owned_session_facts(&source).unwrap();
    let session = &source.snapshot.sessions[0];
    assert!(matches!(
        facts[&session.id].tabs[&session.tabs[0].id].lifecycle,
        wire::TabLifecycle::Open
    ));
    source.terminals[0].phase = Phase::Closed;
    let facts = project_owned_session_facts(&source).unwrap();
    let dto = adapter::session_dto(session, &facts[&session.id]).unwrap();
    assert!(matches!(dto.tabs[0].lifecycle, wire::TabLifecycle::Closed));
    assert_eq!(dto.counts.closed, 1);
    source.terminals.clear();
    let facts = project_owned_session_facts(&source).unwrap();
    assert!(facts[&session.id].tabs[&session.tabs[0].id]
        .generation
        .is_none());
    assert!(facts[&session.id].terminal_lifetime.is_none());
}
