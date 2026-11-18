use shellfox_core::Settings;
use shellfox_service::activity::*;
use std::cmp::Ordering;

fn observation(agents: u32, health: Health) -> Observation {
    Observation {
        root: RootState::Alive,
        health,
        agents,
        reason: None,
        observed_at_ms: 0,
    }
}
fn tab(status: TabStatus, busy: bool) -> TabFacts {
    TabFacts {
        lifecycle: TabLifecycle::Open,
        status,
        agents: u32::from(status == TabStatus::Running),
        busy,
        error: false,
    }
}

#[test]
fn only_owned_evidence_can_report_an_agent_and_busy_is_separate() {
    assert_eq!(
        tab_status(TabLifecycle::Open, false, None),
        TabStatus::Unknown
    );
    assert_eq!(
        tab_status(
            TabLifecycle::Open,
            false,
            Some(&observation(0, Health::Healthy))
        ),
        TabStatus::Waiting
    );
    assert_eq!(
        tab_status(
            TabLifecycle::Open,
            false,
            Some(&observation(0, Health::Unknown))
        ),
        TabStatus::Unknown
    );
    assert_eq!(
        tab_status(
            TabLifecycle::Open,
            false,
            Some(&observation(1, Health::Unknown))
        ),
        TabStatus::Running
    );
    assert_eq!(
        tab_status(
            TabLifecycle::Closed,
            false,
            Some(&observation(1, Health::Healthy))
        ),
        TabStatus::Unknown
    );
    assert_eq!(
        tab_status(TabLifecycle::Closed, true, None),
        TabStatus::Error
    );
    assert_eq!(
        tab_agent_state(&tab(TabStatus::Waiting, true)),
        AgentState::Shell
    );
    assert_eq!(
        tab_agent_state(&tab(TabStatus::Running, false)),
        AgentState::Running
    );
    assert_eq!(
        tab_agent_state(&tab(TabStatus::Running, true)),
        AgentState::Busy
    );
}

#[test]
fn aggregation_counts_closed_errors_but_not_closed_agents_and_settlement_overrides_dot() {
    let mut closed = tab(TabStatus::Error, true);
    closed.lifecycle = TabLifecycle::Closed;
    closed.agents = 99;
    let facts = session_facts(&[tab(TabStatus::Running, true), closed], false, false);
    assert_eq!(facts.status, Status::Error);
    assert_eq!(facts.activity_status, TabStatus::Error);
    assert_eq!(facts.agent_state, AgentState::Busy);
    assert_eq!(
        facts.counts,
        Counts {
            running: 1,
            error: 1,
            closed: 1,
            agents: 1,
            ..Counts::default()
        }
    );
    let settled = session_facts(&[tab(TabStatus::Running, true)], true, true);
    assert_eq!(settled.status, Status::Settled);
    assert_eq!(settled.activity_status, TabStatus::Error);
    assert_eq!(settled.agent_state, AgentState::Settled);
    assert_eq!(session_facts(&[], false, false).status, Status::Unknown);
    assert_eq!(
        session_facts(
            &[
                tab(TabStatus::Waiting, false),
                tab(TabStatus::Unknown, false)
            ],
            false,
            false
        )
        .activity_status,
        TabStatus::Unknown
    );
}

#[test]
fn sort_priorities_match_protocol_and_visible_dot_orders_including_pins() {
    let mut idle = SessionSortKey {
        pinned_at: None,
        created_at: "1",
        id: "a",
        status: Status::Running,
        agent_state: AgentState::Running,
    };
    let mut busy = SessionSortKey {
        pinned_at: None,
        created_at: "1",
        id: "b",
        status: Status::Running,
        agent_state: AgentState::Busy,
    };
    assert_eq!(compare_visible_sessions(&idle, &busy), Ordering::Less);
    busy.status = Status::Error;
    busy.agent_state = AgentState::Error;
    assert_eq!(compare_visible_sessions(&idle, &busy), Ordering::Greater);
    assert_eq!(compare_sessions(&idle, &busy), Ordering::Less);
    idle.pinned_at = Some("2");
    busy.pinned_at = Some("1");
    assert_eq!(compare_sessions(&idle, &busy), Ordering::Greater);
    busy.pinned_at = None;
    assert_eq!(compare_visible_sessions(&idle, &busy), Ordering::Less);
}

#[test]
fn inactivity_settles_without_closing_then_gives_a_full_delete_grace() {
    let settings = Settings {
        session_settle_hours: 1.0,
        session_delete_hours: 2.0,
        ..Settings::default()
    };
    let mut activity = SessionActivity::new();
    let mut session = SessionAging {
        updated_ms: 0,
        settled_ms: None,
        pinned: false,
    };
    assert_eq!(
        activity.expiry(session, &settings, 3_600_000, false),
        ExpiryAction::None
    );
    assert_eq!(
        activity.expiry(session, &settings, 3_600_000, true),
        ExpiryAction::Settle
    );
    session.settled_ms = Some(3_600_000);
    assert_eq!(
        activity.expiry(session, &settings, 10_799_999, true),
        ExpiryAction::None
    );
    assert_eq!(
        activity.expiry(session, &settings, 10_800_000, true),
        ExpiryAction::Delete
    );
    activity.touch(10_800_000);
    assert_eq!(
        activity.expiry(session, &settings, 10_800_000, true),
        ExpiryAction::None
    );
    assert_eq!(
        activity.expiry(session, &settings, 18_000_000, true),
        ExpiryAction::Delete
    );
    session.pinned = true;
    assert_eq!(
        activity.expiry(session, &settings, 90_000_000, true),
        ExpiryAction::None
    );
}

#[test]
fn touches_during_expiry_refresh_and_failed_persistence_are_not_lost() {
    let settings = Settings {
        session_settle_hours: 1.0,
        ..Settings::default()
    };
    let mut activity = SessionActivity::new();
    let session = SessionAging {
        updated_ms: 0,
        settled_ms: None,
        pinned: false,
    };
    assert!(activity.due(session, &settings, 3_600_000));
    activity.touch(3_600_000);
    assert_eq!(
        activity.expiry(session, &settings, 3_600_000, true),
        ExpiryAction::None
    );
    assert_eq!(activity.pending_update(0), Some(3_600_000));
    activity.touch(3_600_001);
    activity.persisted(3_600_000);
    assert_eq!(activity.pending_update(3_600_000), Some(3_600_001));
    activity.persisted(3_600_001);
    assert_eq!(activity.pending_update(3_600_001), None);
    assert!(!activity.due(
        session,
        &Settings {
            session_settle_hours: f64::NAN,
            ..settings
        },
        3_600_000
    ));
}

#[test]
fn manual_settlement_tokens_never_collide_on_fixed_or_backwards_clock() {
    let mut clock = SettlementClock::default();
    clock.seed(1000);
    assert_eq!(clock.next(1000), Some(1001));
    assert_eq!(clock.next(999), Some(1002));
    assert_eq!(clock.next(2000), Some(2000));
    clock.seed(u64::MAX);
    assert_eq!(clock.next(3000), None);
}
