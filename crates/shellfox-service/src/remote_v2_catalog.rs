use super::*;
use v2::{CatalogEntry, Fact, UnknownReason};
pub(super) fn unknown<T>() -> Fact<T> {
    Fact::Unknown {
        reason: UnknownReason::Unavailable,
    }
}
pub(super) fn known<T>(value: T) -> Fact<T> {
    Fact::Known { value }
}
pub(super) fn id(value: impl std::fmt::Display) -> Result<v2::Id, AppError> {
    value.to_string().parse().map_err(|_| {
        failure(
            ErrorCode::StorageFailed,
            "Stored identity is not representable.",
        )
    })
}
pub(super) fn entry(
    state: &RemoteState,
    service: &Service,
    session: &shellfox_core::Session,
) -> Result<CatalogEntry, AppError> {
    entry_with_creation(state, service, session, &Creation::read(service))
}

struct Creation {
    shell: Option<shellfox_host::SessionCreationFactMap>,
    reserved: Option<std::collections::HashSet<SessionId>>,
}
impl Creation {
    fn read(service: &Service) -> Self {
        Self {
            shell: service.host().session_creation_facts().ok(),
            reserved: service.host().reserved_publication_sessions().ok(),
        }
    }

    fn missing<T>(&self, id: SessionId) -> Fact<T> {
        match (&self.shell, &self.reserved) {
            // Reserved publication records intentional absence, unlike old
            // sessions whose shell creation was never recorded.
            (Some(_), Some(rows)) if rows.contains(&id) => Fact::NotApplicable,
            (Some(_), Some(_)) => Fact::Unknown { reason: UnknownReason::NotRecorded },
            _ => unknown(),
        }
    }
}

fn entry_with_creation(
    state: &RemoteState, service: &Service, session: &shellfox_core::Session, creation: &Creation,
) -> Result<CatalogEntry, AppError> {
    // Actor manager permission authorizes saved session metadata. Tab visibility
    // is applied before any tab facts or diagnostics, never a whole-row filter.
    let session_id = id(session.id)?;
    match project(state, service, session, creation) {
        Ok(value) if value.validate().is_ok() => Ok(CatalogEntry::Session {
            value: Box::new(value),
        }),
        _ => Ok(CatalogEntry::Unavailable {
            session_id,
            issues: vec![v2::ProjectionIssue {
                reason: v2::ProjectionReason::InvalidMetadata,
                fields: vec![
                    v2::FactField::Cwd,
                    v2::FactField::CreatedAt,
                    v2::FactField::Status,
                    v2::FactField::Runtime,
                ],
            }],
        }),
    }
}
fn project(state: &RemoteState, service: &Service, session: &shellfox_core::Session, birth: &Creation) -> Result<v2::Session, AppError> {
    let mut header = session.clone(); header.tabs.clear();
    header.validate().map_err(host_error)?;
    let creation = birth.shell.as_ref().and_then(|rows| rows.get(&session.id));
    let adapter_id = creation
        .map(|facts| {
            known(match facts.adapter_id {
                shellfox_core::SettingsAdapter::EmbeddedPty => wire::AdapterId::EmbeddedPty,
                shellfox_core::SettingsAdapter::GnomeTerminal => wire::AdapterId::GnomeTerminal,
                shellfox_core::SettingsAdapter::WindowsTerminal => wire::AdapterId::WindowsTerminal,
            })
        })
        .unwrap_or_else(|| birth.missing(session.id));
    let shell_id = creation
        .map(|facts| {
            known(match facts.shell_id {
                shellfox_core::SettingsShell::Pwsh => wire::ShellId::Pwsh,
                shellfox_core::SettingsShell::WindowsPowershell => wire::ShellId::WindowsPowershell,
                shellfox_core::SettingsShell::Bash => wire::ShellId::Bash,
                shellfox_core::SettingsShell::LoginShell => wire::ShellId::LoginShell,
                shellfox_core::SettingsShell::Wsl => wire::ShellId::Wsl,
            })
        })
        .unwrap_or_else(|| birth.missing(session.id));
    let control_reason = if matches!(adapter_id, Fact::NotApplicable) {
        "Automation replay is local-only."
    } else {
        "observer-v2 provides read-only delivery on the canonical grid."
    };
    let tabs = session
        .tabs
        .iter()
        .filter(|tab| tab_visible(state, service, tab))
        .map(|tab| tabs::project(service, tab))
        .collect::<Result<Vec<_>, _>>()?;
    let startup_error = tabs.iter().find_map(|tab| tab.facts.error.known().and_then(Option::as_ref).filter(|error| error.code == ErrorCode::LaunchFailed)).cloned();
    let activity = if startup_error.is_some() { wire::TabStatus::Error } else { aggregate(&tabs) };
    let status = if session.settled_at.is_some() {
        wire::Status::Settled
    } else {
        match activity {
            wire::TabStatus::Running => wire::Status::Running,
            wire::TabStatus::Waiting => wire::Status::Waiting,
            wire::TabStatus::Error => wire::Status::Error,
            wire::TabStatus::Unknown => wire::Status::Unknown,
        }
    };
    Ok(v2::Session {
        id: id(session.id)?,
        title: session.title.clone(),
        facts: v2::SessionFacts {
            cwd: known(session.cwd.clone()),
            adapter_id,
            shell_id,
            created_at: known(session.created_at.clone()),
            updated_at: known(session.updated_at.clone()),
            settled_at: known(session.settled_at.clone()),
            pinned_at: known(session.pinned_at.clone()),
            status: known(status),
            activity_status: known(activity),
            counts: counts(&tabs),
            env: known(session.env.clone()),
            window: unknown(),
            can_focus: known(false),
            can_add_tab: unknown(),
            control_reason: known(Some(control_reason.into())),
            error: startup_error.map(|error| known(Some(error))).unwrap_or_else(unknown),
            terminal_lifetime: unknown(),
            shell_survival: unknown(),
        },
        tabs,
    })
}
fn aggregate(tabs: &[v2::Tab]) -> wire::TabStatus {
    let statuses = tabs
        .iter()
        .filter(|tab| {
            !matches!(
                tab.facts.lifecycle.known(),
                Some(wire::TabLifecycle::Closed)
            )
        })
        .map(|tab| {
            tab.facts
                .status
                .known()
                .copied()
                .unwrap_or(wire::TabStatus::Unknown)
        })
        .collect::<Vec<_>>();
    if statuses.iter().any(|s| matches!(s, wire::TabStatus::Error)) {
        wire::TabStatus::Error
    } else if statuses
        .iter()
        .any(|s| matches!(s, wire::TabStatus::Running))
    {
        wire::TabStatus::Running
    } else if statuses.is_empty()
        || statuses
            .iter()
            .any(|s| matches!(s, wire::TabStatus::Unknown))
    {
        wire::TabStatus::Unknown
    } else {
        wire::TabStatus::Waiting
    }
}
fn counts(tabs: &[v2::Tab]) -> Fact<wire::Counts> {
    let mut counts = wire::Counts {
        running: 0,
        waiting: 0,
        unknown: 0,
        error: 0,
        closed: 0,
        agents: 0,
    };
    for tab in tabs {
        let (Some(life), Some(status), Some(agents)) = (
            tab.facts.lifecycle.known(),
            tab.facts.status.known(),
            tab.facts.agents.known(),
        ) else {
            return unknown();
        };
        let closed = matches!(life, wire::TabLifecycle::Closed);
        counts.closed += u64::from(closed);
        if !closed || matches!(status, wire::TabStatus::Error) {
            match status {
                wire::TabStatus::Running => counts.running += 1,
                wire::TabStatus::Waiting => counts.waiting += 1,
                wire::TabStatus::Unknown => counts.unknown += 1,
                wire::TabStatus::Error => counts.error += 1,
            }
        }
        if !closed {
            let Some(total) = counts.agents.checked_add(*agents) else {
                return unknown();
            };
            counts.agents = total;
        }
    }
    known(counts)
}
pub(super) fn snapshot(state: &RemoteState, service: &Service) -> Result<v2::Snapshot, AppError> {
    let snapshot = service.host().snapshot().map_err(host_error)?;
    let creation = Creation::read(service);
    let mut entries = Vec::with_capacity(snapshot.sessions.len());
    let mut encoded_bytes = 0usize;
    for session in &snapshot.sessions {
        let entry = entry_with_creation(state, service, session, &creation)?;
        let size = serde_json::to_vec(&entry).map_err(|_| failure(ErrorCode::Internal, "Invalid catalog entry."))?.len();
        encoded_bytes = encoded_bytes.checked_add(size).ok_or_else(|| failure(ErrorCode::Unsupported, "Complete authorized inventory exceeds its response bound."))?;
        if encoded_bytes > wire::MAX_RESPONSE_BYTES { return Err(failure(ErrorCode::Unsupported, "Complete authorized inventory exceeds its response bound; no partial rows returned.")); }
        entries.push(entry);
    }
    let settings = service.host().global_settings().map_err(host_error)?;
    Ok(v2::Snapshot {
        revision: snapshot.revision,
        coverage: v2::Coverage {
            mode: v2::CatalogMode::Complete,
            entry_count: entries.len(),
        },
        entries,
        settings: known(super::settings::project(&settings)),
        probe: state.facts.probe.clone().map(known).unwrap_or_else(unknown),
        explorer: state
            .facts
            .explorer
            .clone()
            .map(known)
            .unwrap_or_else(unknown),
        cli: state.facts.cli.clone().map(known).unwrap_or_else(unknown),
    })
}
