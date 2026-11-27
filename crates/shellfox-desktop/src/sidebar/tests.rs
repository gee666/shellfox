use super::*;
use shellfox_runtime::{Identity, Phase};
use shellfox_service::{
    NativeTabActivity,
    activity::integration::{StartupEvidence, TerminalSourceFacts},
    activity::{Health, Observation, RootState},
};

#[test]
fn pin_age_then_visible_status_then_creation_time_orders_sessions() {
    let sessions: Vec<_> = (0..7)
        .map(|n| Session {
            id: SessionId::new(),
            title: n.to_string(),
            cwd: "/demo".into(),
            created_at: n.to_string(),
            updated_at: n.to_string(),
            settled_at: None,
            pinned_at: match n {
                5 => Some("2".into()),
                6 => Some("1".into()),
                _ => None,
            },
            env: vec![],
            tabs: vec![],
        })
        .collect();
    let rows = sort_sessions(&sessions, |session| match session.title.as_str() {
        "0" => AgentState::Shell,
        "1" => AgentState::Settled,
        "2" => AgentState::Busy,
        "3" => AgentState::Running,
        _ => AgentState::Error,
    });
    assert_eq!(
        rows.iter()
            .map(|(session, _)| session.title.as_str())
            .collect::<Vec<_>>(),
        ["6", "5", "4", "3", "2", "0", "1"]
    );
}

#[test]
fn idle_sorts_above_working_when_both_hosts_facts_contain_a_live_agent() {
    let sessions = ["Working", "Idle"].map(|name| Session {
        id: SessionId::new(),
        title: name.into(),
        cwd: "/demo".into(),
        created_at: if name == "Working" { "1" } else { "2" }.into(),
        updated_at: "3".into(),
        settled_at: None,
        pinned_at: None,
        env: vec![],
        tabs: vec![],
    });
    let rows = sort_sessions(&sessions, |session| {
        let source = TerminalSourceFacts {
            identity: Identity {
                session_id: session.id,
                tab_id: Default::default(),
                generation: Default::default(),
            },
            phase: Phase::Active,
            startup: StartupEvidence::Confirmed,
            source_kind: None,
            automation_exit_code: None,
            observation: Some(Observation {
                root: RootState::Alive,
                agents: 1,
                health: Health::Healthy,
                reason: None,
                observed_at_ms: 10_000,
            }),
            busy: session.title == "Working",
            activity_available: true,
            title: None,
        };
        NativeTabActivity::from(&source).dot
    });
    let display: Vec<_> = rows
        .into_iter()
        .map(|(session, dot)| (session.title.as_str(), super::row::status_label(dot, None)))
        .collect();
    assert_eq!(
        display,
        [
            ("Idle", "Agent idle".into()),
            ("Working", "Agent working".into())
        ]
    );
}
