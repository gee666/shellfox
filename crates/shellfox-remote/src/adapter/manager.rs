use super::*;
use wire::{Status, TabLifecycle, TabStatus};

pub fn settings_dto(
    preferences: &core::Preferences,
    facts: &SettingsFacts,
) -> AdapterResult<wire::SettingsDto> {
    preferences
        .validate()
        .map_err(|_| AdapterError::InvalidFacts {
            context: "preferences",
        })?;
    checked(
        wire::SettingsDto {
            version: 1,
            python_path: None,
            accent_color: preferences.accent_color.clone(),
            background_color: preferences.background_color.clone(),
            adapter_id: facts.adapter_id,
            shell_id: facts.shell_id,
            shell_executable: facts.shell_executable.clone(),
            process_rules: facts.process_rules.clone(),
            history_page_size: facts.history_page_size,
            session_settle_hours: facts.session_settle_hours,
            session_delete_hours: facts.session_delete_hours,
            auto_update: facts.auto_update,
            terminal_profile_id: Some(Some(preferences.default_profile_id.to_string())),
            terminal_env: facts.terminal_env.clone(),
        },
        "settings",
    )
}

pub fn manager_snapshot(
    snapshot: &core::Snapshot,
    facts: &ManagerFacts,
) -> AdapterResult<wire::ManagerSnapshot> {
    manager_snapshot_visible(snapshot, facts, &Default::default())
}
pub fn manager_snapshot_visible(
    snapshot: &core::Snapshot,
    facts: &ManagerFacts,
    private: &std::collections::HashSet<core::TabId>,
) -> AdapterResult<wire::ManagerSnapshot> {
    let settings = settings_dto(
        &snapshot.preferences,
        required(facts.settings.as_ref(), "manager settings")?,
    )?;
    let probe = required(facts.probe.as_ref(), "native probe")?.clone();
    let explorer = required(facts.explorer.as_ref(), "explorer integration")?.clone();
    let cli = required(facts.cli.as_ref(), "CLI integration")?.clone();
    let mut ids = std::collections::HashSet::new();
    let mut tab_ids = std::collections::HashSet::new();
    let mut sessions = Vec::with_capacity(snapshot.sessions.len());
    for session in &snapshot.sessions {
        ensure(ids.insert(session.id), "duplicate session identity")?;
        for tab in &session.tabs {
            ensure(tab_ids.insert(tab.id), "duplicate tab identity")?;
        }
        if facts.omitted_sessions.contains(&session.id) {
            ensure(
                !facts.sessions.contains_key(&session.id),
                "omitted session has conflicting facts",
            )?;
            continue;
        }
        sessions.push(session_dto_visible(
            session,
            required(facts.sessions.get(&session.id), "session runtime facts")?,
            private,
        )?);
    }
    checked(
        wire::ManagerSnapshot {
            revision: snapshot.revision,
            sessions,
            settings,
            probe,
            explorer,
            cli,
        },
        "manager snapshot",
    )
}

/// Use before a mutation to reject unavailable projections without committing it.
/// New tabs still need their own facts, including their actual creation time.
pub fn validate_session_facts(session: &core::Session, facts: &SessionFacts) -> AdapterResult<()> {
    session_dto(session, facts).map(|_| ())
}

pub fn session_dto(
    session: &core::Session,
    facts: &SessionFacts,
) -> AdapterResult<wire::SessionDto> {
    session_dto_visible(session, facts, &Default::default())
}
pub fn session_dto_visible(
    session: &core::Session,
    facts: &SessionFacts,
    private: &std::collections::HashSet<core::TabId>,
) -> AdapterResult<wire::SessionDto> {
    if private.is_empty() {
        session.validate().map_err(|_| AdapterError::InvalidFacts {
            context: "session metadata",
        })?;
    } else {
        let mut header = session.clone();
        header.tabs.clear();
        header.validate().map_err(|_| AdapterError::InvalidFacts {
            context: "session metadata",
        })?;
    }
    wire_identity(session.id)?;
    wire_timestamp(&session.created_at)?;
    wire_timestamp(&session.updated_at)?;
    optional_wire_timestamp(session.settled_at.as_deref())?;
    optional_wire_timestamp(session.pinned_at.as_deref())?;
    let tabs = session
        .tabs
        .iter()
        .filter(|tab| !private.contains(&tab.id))
        .map(|tab| tab_dto(tab, required(facts.tabs.get(&tab.id), "tab runtime facts")?))
        .collect::<AdapterResult<Vec<_>>>()?;
    let activity_status = aggregate_status(&tabs, facts.error.is_some());
    let status = if session.settled_at.is_some() {
        Status::Settled
    } else {
        match activity_status {
            TabStatus::Waiting => Status::Waiting,
            TabStatus::Error => Status::Error,
            TabStatus::Running => Status::Running,
            TabStatus::Unknown => Status::Unknown,
        }
    };
    let counts = count_tabs(&tabs);
    checked(
        wire::SessionDto {
            id: session.id,
            title: session.title.clone(),
            cwd: session.cwd.clone(),
            adapter_id: facts.adapter_id,
            shell_id: facts.shell_id,
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            settled_at: session.settled_at.clone(),
            pinned_at: session.pinned_at.clone(),
            status,
            activity_status,
            counts,
            tabs,
            env: session.env.clone(),
            window: facts.window.clone(),
            can_focus: facts.can_focus,
            can_add_tab: facts.can_add_tab,
            control_reason: facts.control_reason.clone(),
            error: facts.error.clone(),
            terminal_lifetime: facts.terminal_lifetime,
            shell_survival: facts.shell_survival,
        },
        "session DTO",
    )
}

pub fn tab_dto(tab: &core::Tab, facts: &TabFacts) -> AdapterResult<wire::TabDto> {
    wire_identity(tab.id)?;
    wire_identity(tab.session_id)?;
    if let Some(generation) = facts.generation {
        wire_identity(generation)?;
    }
    safe_integer(facts.exit_code.flatten())?;
    wire_timestamp(&facts.created_at)?;
    if let Some(created_at) = &tab.created_at {
        ensure(
            created_at == &facts.created_at,
            "tab creation timestamp differs from persisted evidence",
        )?;
    }
    ensure(
        !matches!(tab.state, core::TabState::Archived)
            || matches!(facts.lifecycle, TabLifecycle::Closed),
        "archived tab has live lifecycle",
    )?;
    let (agents, monitoring_reason, observed_status) = match &facts.observation {
        ObservationFacts::Unavailable { reason } => {
            ensure(!reason.trim().is_empty(), "missing monitoring reason")?;
            (0, Some(reason.clone()), TabStatus::Unknown)
        }
        ObservationFacts::Exited => (0, None, TabStatus::Unknown),
        ObservationFacts::Alive {
            agents,
            healthy,
            reason,
        } => {
            ensure(
                matches!(facts.lifecycle, TabLifecycle::Open),
                "alive observation requires open lifecycle",
            )?;
            let status = if *agents > 0 {
                TabStatus::Running
            } else if *healthy {
                TabStatus::Waiting
            } else {
                TabStatus::Unknown
            };
            (*agents, reason.clone(), status)
        }
    };
    checked(
        wire::TabDto {
            id: tab.id,
            session_id: tab.session_id,
            title: match &facts.process_title {
                Some(title) if tab.use_process_title => title.clone(),
                _ => tab.title.clone(),
            },
            cwd: tab.cwd.clone(),
            ordinal: u64::from(tab.ordinal),
            created_at: facts.created_at.clone(),
            lifecycle: facts.lifecycle,
            status: if facts.error.is_some() {
                TabStatus::Error
            } else {
                observed_status
            },
            agents,
            monitoring_reason,
            error: facts.error.clone(),
            generation: facts.generation,
            terminal_kind: facts.terminal_kind,
            profile_id: Some(Some(tab.profile_id.to_string())),
            exit_code: facts.exit_code,
        },
        "tab DTO",
    )
}

/// Filter with host-proved non-elevated profile IDs, then recompute derived
/// counts/status. Unknown profiles do not acquire remote authority by omission.
pub fn filter_session_profiles(
    mut session: wire::SessionDto,
    allowed: &std::collections::HashSet<String>,
) -> AdapterResult<wire::SessionDto> {
    session.tabs.retain(|tab| {
        tab.profile_id
            .as_ref()
            .and_then(Option::as_ref)
            .is_some_and(|id| allowed.contains(id))
    });
    finish_visible_session(session)
}
/// Exclude only tab identities the existing manager policy marks local-only.
/// Authorized unknown metadata is not an allowlist miss or a partial inventory.
pub fn filter_private_tabs(
    mut session: wire::SessionDto,
    private: &std::collections::HashSet<core::TabId>,
) -> AdapterResult<wire::SessionDto> {
    session.tabs.retain(|tab| !private.contains(&tab.id));
    finish_visible_session(session)
}
fn finish_visible_session(mut session: wire::SessionDto) -> AdapterResult<wire::SessionDto> {
    session.counts = count_tabs(&session.tabs);
    session.activity_status = aggregate_status(&session.tabs, session.error.is_some());
    if session.settled_at.is_none() {
        session.status = match session.activity_status {
            TabStatus::Waiting => Status::Waiting,
            TabStatus::Error => Status::Error,
            TabStatus::Running => Status::Running,
            TabStatus::Unknown => Status::Unknown,
        };
    }
    if session.tabs.is_empty() {
        session.can_focus = false;
    }
    checked(session, "remote-visible session DTO")
}

// Match src/shared/status.ts: closed errors count, closed agents do not.
fn count_tabs(tabs: &[wire::TabDto]) -> wire::Counts {
    let mut counts = wire::Counts {
        running: 0,
        waiting: 0,
        unknown: 0,
        error: 0,
        closed: 0,
        agents: 0,
    };
    for tab in tabs {
        let closed = matches!(tab.lifecycle, TabLifecycle::Closed);
        counts.closed += u64::from(closed);
        if !closed || matches!(tab.status, TabStatus::Error) {
            match tab.status {
                TabStatus::Running => counts.running += 1,
                TabStatus::Waiting => counts.waiting += 1,
                TabStatus::Unknown => counts.unknown += 1,
                TabStatus::Error => counts.error += 1,
            }
        }
        if !closed {
            counts.agents += tab.agents;
        }
    }
    counts
}
fn aggregate_status(tabs: &[wire::TabDto], error: bool) -> TabStatus {
    if error || tabs.iter().any(|t| matches!(t.status, TabStatus::Error)) {
        return TabStatus::Error;
    }
    let live: Vec<_> = tabs
        .iter()
        .filter(|t| !matches!(t.lifecycle, TabLifecycle::Closed))
        .collect();
    if live.iter().any(|t| matches!(t.status, TabStatus::Running)) {
        TabStatus::Running
    } else if live.is_empty() || live.iter().any(|t| matches!(t.status, TabStatus::Unknown)) {
        TabStatus::Unknown
    } else {
        TabStatus::Waiting
    }
}
