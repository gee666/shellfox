//! Project validated v1 metadata into the sidebar's catalog model. A legacy
//! snapshot is not evidence for a Manager-v2 source kind or binding.
use shellfox_protocol::{self as wire, Validate, manager_v2 as m};

fn known<T>(value: T) -> m::Fact<T> {
    m::Fact::Known { value }
}
fn optional<T>(value: Option<T>) -> m::Fact<T> {
    value.map(known).unwrap_or(m::Fact::Unknown {
        reason: m::UnknownReason::NotRecorded,
    })
}

pub(super) fn project(snapshot: wire::ManagerSnapshot) -> anyhow::Result<m::Snapshot> {
    snapshot.validate()?;
    let entries = snapshot
        .sessions
        .into_iter()
        .map(session)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let settings = snapshot.settings;
    let settings = m::Settings {
        version: settings.version,
        python_path: None,
        accent_color: settings.accent_color,
        background_color: Some(settings.background_color),
        adapter_id: settings.adapter_id,
        shell_id: settings.shell_id,
        shell_executable: settings.shell_executable,
        process_rules: settings
            .process_rules
            .into_iter()
            .map(|rule| wire::observer_v2::ProcessRule {
                id: rule.id,
                label: rule.label,
                enabled: rule.enabled,
                executable_basenames: rule.executable_basenames,
                executable_paths: rule.executable_paths,
                script_path_suffixes: rule.script_path_suffixes,
                process_names: rule.process_names,
            })
            .collect(),
        history_page_size: Some(settings.history_page_size),
        session_settle_hours: Some(settings.session_settle_hours),
        session_delete_hours: Some(settings.session_delete_hours),
        auto_update: Some(settings.auto_update),
        terminal_profile_id: settings.terminal_profile_id,
        terminal_env: Some(settings.terminal_env),
    };
    Ok(m::Snapshot {
        revision: snapshot.revision,
        coverage: wire::observer_v2::Coverage {
            mode: m::CatalogMode::Complete,
            entry_count: entries.len(),
        },
        entries,
        settings: known(settings),
        probe: known(snapshot.probe),
        explorer: known(snapshot.explorer),
        cli: known(snapshot.cli),
    })
}

fn session(session: wire::SessionDto) -> anyhow::Result<m::CatalogEntry> {
    let tabs = session
        .tabs
        .into_iter()
        .map(tab)
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(m::CatalogEntry::Session {
        value: Box::new(m::Session {
            id: session.id.to_string().parse()?,
            title: session.title,
            tabs,
            facts: m::SessionFacts {
                cwd: known(session.cwd),
                adapter_id: known(session.adapter_id),
                shell_id: known(session.shell_id),
                created_at: known(session.created_at),
                updated_at: known(session.updated_at),
                settled_at: known(session.settled_at),
                pinned_at: known(session.pinned_at),
                status: known(session.status),
                activity_status: known(session.activity_status),
                counts: known(session.counts),
                env: known(session.env),
                window: optional(session.window.map(Some)),
                can_focus: known(session.can_focus),
                can_add_tab: known(session.can_add_tab),
                control_reason: known(session.control_reason),
                error: known(session.error),
                terminal_lifetime: optional(session.terminal_lifetime.map(Some)),
                shell_survival: optional(session.shell_survival.map(Some)),
            },
        }),
    })
}

fn tab(tab: wire::TabDto) -> anyhow::Result<m::Tab> {
    let generation = tab
        .generation
        .map(|id| id.to_string().parse())
        .transpose()?;
    Ok(m::Tab {
        id: tab.id.to_string().parse()?,
        session_id: tab.session_id.to_string().parse()?,
        title: tab.title,
        source_kind: m::Fact::Unknown {
            reason: m::UnknownReason::NotRecorded,
        },
        facts: m::TabFacts {
            cwd: known(tab.cwd),
            ordinal: known(tab.ordinal),
            created_at: known(tab.created_at),
            lifecycle: known(tab.lifecycle),
            status: known(tab.status),
            agents: known(tab.agents),
            monitoring_reason: known(tab.monitoring_reason),
            error: known(tab.error),
            generation: optional(generation.map(Some)),
            terminal_kind: optional(tab.terminal_kind.map(Some)),
            profile_id: optional(tab.profile_id),
            exit_code: optional(tab.exit_code),
        },
    })
}
