//! Pure sanitized v1 session mapping. Discovery/system orchestration has one
//! owner: activity_integration::NativeSourceFactsProjector.
#[path = "facts_tabs.rs"]
mod tabs;
#[cfg(test)]
#[path = "facts_projection_tests.rs"]
mod tests;
use crate::activity_integration::SourceFacts;
use shellfox_protocol::{self as wire, AppError, ErrorCode};
use shellfox_remote::adapter::SessionFacts;
use std::collections::{HashMap, HashSet};

/// No Host/Runtime calls, discovery, settings fallback or fabricated process
/// state. Creation intent and current owned observations remain distinct.
pub fn project_session_facts(
    source: &SourceFacts,
) -> Result<HashMap<shellfox_core::SessionId, SessionFacts>, AppError> {
    project_session_facts_for(source, None)
}

/// An internal response scope, not a partial inventory. The full owner snapshot
/// remains unchanged and fenced; only an operation's required session is mapped.
pub fn project_session_facts_for(
    source: &SourceFacts,
    target: Option<shellfox_core::SessionId>,
) -> Result<HashMap<shellfox_core::SessionId, SessionFacts>, AppError> {
    project_session_facts_visible(source, target, &Default::default())
}
pub fn project_session_facts_visible(
    source: &SourceFacts,
    target: Option<shellfox_core::SessionId>,
    private: &HashSet<shellfox_core::TabId>,
) -> Result<HashMap<shellfox_core::SessionId, SessionFacts>, AppError> {
    project(source, target, private, false).map(|projection| projection.sessions)
}

#[derive(Default)]
pub(crate) struct LegacyProjection {
    pub(crate) sessions: HashMap<shellfox_core::SessionId, SessionFacts>,
    pub(crate) omitted_sessions: HashSet<shellfox_core::SessionId>,
}

/// V1 has no unavailable-entry envelope. Omit only rows the owner knows
/// cannot be represented; targeted operations still receive their error.
pub(crate) fn project_legacy_catalog(
    source: &SourceFacts,
    target: Option<shellfox_core::SessionId>,
    private: &HashSet<shellfox_core::TabId>,
) -> Result<LegacyProjection, AppError> {
    project(source, target, private, target.is_none())
}

fn project(
    source: &SourceFacts,
    target: Option<shellfox_core::SessionId>,
    private: &HashSet<shellfox_core::TabId>,
    omit_unsupported: bool,
) -> Result<LegacyProjection, AppError> {
    if target.is_some_and(|id| {
        !source
            .snapshot
            .sessions
            .iter()
            .any(|session| session.id == id)
    }) {
        return Err(AppError {
            code: ErrorCode::NotFound,
            message: "Target session is not in the captured owner snapshot.".into(),
            retryable: false,
        });
    }
    if source.cancelled.is_cancelled() {
        return Err(unavailable("Owned session facts were cancelled."));
    }
    let mut projection = LegacyProjection::default();
    for session in source
        .snapshot
        .sessions
        .iter()
        .filter(|session| target.is_none_or(|id| session.id == id))
    {
        match project_session(source, session, private) {
            Ok(facts) => {
                projection.sessions.insert(session.id, facts);
            }
            Err(error) if omit_unsupported && error.code == ErrorCode::Unsupported => {
                projection.omitted_sessions.insert(session.id);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(projection)
}

fn project_session(
    source: &SourceFacts,
    session: &shellfox_core::Session,
    private: &HashSet<shellfox_core::TabId>,
) -> Result<SessionFacts, AppError> {
    let mut header = session.clone();
    if !private.is_empty() {
        header.tabs.clear();
    }
    header
        .validate()
        .map_err(|_| unavailable("Session metadata is invalid."))?;
    let replay = session.tabs.iter().filter(|tab| !private.contains(&tab.id)).any(|tab| {
        source.terminals.iter().any(|terminal| terminal.identity.session_id == session.id
            && terminal.identity.tab_id == tab.id && terminal.automation_exit_code.is_some())
    });
    let (adapter_id, shell_id) = if replay {
        // The legacy wire format requires these fields. Match Electron's
        // automation DTO without recording a shell that was never started.
        (wire::AdapterId::EmbeddedPty, wire::ShellId::LoginShell)
    } else {
        let creation = source.creation_facts.get(&session.id).ok_or_else(|| {
            incompatible("V1 requires recorded session creation adapter/shell facts. No settings/first-tab backfill is allowed.")
        })?;
        creation.validate().map_err(|_| unavailable("Persisted session creation facts are invalid."))?;
        (adapter(creation.adapter_id), shell(creation.shell_id))
    };
    let tab_facts: HashMap<_, _> = session
        .tabs
        .iter()
        .filter(|tab| !private.contains(&tab.id))
        .map(|tab| {
            let runtime = source
                .terminals
                .iter()
                .find(|terminal| terminal.identity.tab_id == tab.id);
            tabs::project(tab, runtime).map(|facts| (tab.id, facts))
        })
        .collect::<Result<_, _>>()?;
    let owned = source
        .terminals
        .iter()
        .any(|terminal| terminal.identity.session_id == session.id);
    let embedded = matches!(adapter_id, wire::AdapterId::EmbeddedPty);
    let startup_error = session
        .tabs
        .iter()
        .find_map(|tab| {
            tab_facts
                .get(&tab.id)
                .and_then(|facts| facts.error.as_ref())
                .filter(|error| error.code == ErrorCode::LaunchFailed)
        })
        .cloned();
    let facts = SessionFacts {
        adapter_id,
        shell_id,
        window: None,
        can_focus: embedded,
        can_add_tab: embedded && !replay,
        control_reason: replay.then(|| "Automation replay is read-only.".into()),
        error: startup_error,
        terminal_lifetime: owned.then_some(wire::TerminalLifetime::AppOwned),
        shell_survival: owned.then_some(false),
        tabs: tab_facts,
    };
    shellfox_remote::adapter::session_dto_visible(session, &facts, private)
        .map_err(AppError::from)?;
    Ok(facts)
}

fn adapter(value: shellfox_core::SettingsAdapter) -> wire::AdapterId {
    match value {
        shellfox_core::SettingsAdapter::WindowsTerminal => wire::AdapterId::WindowsTerminal,
        shellfox_core::SettingsAdapter::GnomeTerminal => wire::AdapterId::GnomeTerminal,
        shellfox_core::SettingsAdapter::EmbeddedPty => wire::AdapterId::EmbeddedPty,
    }
}
fn shell(value: shellfox_core::SettingsShell) -> wire::ShellId {
    match value {
        shellfox_core::SettingsShell::Pwsh => wire::ShellId::Pwsh,
        shellfox_core::SettingsShell::WindowsPowershell => wire::ShellId::WindowsPowershell,
        shellfox_core::SettingsShell::Bash => wire::ShellId::Bash,
        shellfox_core::SettingsShell::LoginShell => wire::ShellId::LoginShell,
        shellfox_core::SettingsShell::Wsl => wire::ShellId::Wsl,
    }
}
fn incompatible(message: &str) -> AppError {
    AppError {
        code: ErrorCode::Unsupported,
        message: message.into(),
        retryable: false,
    }
}
fn unavailable(message: &str) -> AppError {
    AppError {
        code: ErrorCode::NativeUnavailable,
        message: message.into(),
        retryable: false,
    }
}
