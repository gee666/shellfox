//! Which peer commands wait for owner facts, and for which session.
use super::*;

pub(super) fn required_session(
    command: &OwnerCommand,
    snapshot: &Snapshot,
) -> Result<Option<SessionId>, AppError> {
    use wire::RemoteCommand::*;
    let OwnerCommand::Execute { command, .. } = command else {
        return Ok(None);
    };
    Ok(match command {
        RenameSession(v) => Some(v.session_id),
        RenameTab(v) => Some(v.session_id),
        ReorderTabs(v) => Some(v.session_id),
        SetSessionPinned(v) => Some(v.session_id),
        SettleSession(v) => Some(v.session_id),
        UnsettleSession(v) => Some(v.session_id),
        SetSessionEnv(v) => Some(v.session_id),
        DeleteSession(v) => Some(v.session_id),
        GetSessionStats(v) => v.session_id,
        CloseTab(v) => Some(
            snapshot
                .sessions
                .iter()
                .find(|session| session.tabs.iter().any(|tab| tab.id == v.tab_id))
                .map(|session| session.id)
                .ok_or_else(|| {
                    failure(
                        ErrorCode::NotFound,
                        "Target tab is not in the owner snapshot.",
                    )
                })?,
        ),
        _ => None,
    })
}
pub(super) fn capabilities_only(command: &OwnerCommand) -> bool {
    let OwnerCommand::Execute { command, .. } = command else {
        return false;
    };
    use wire::RemoteCommand::*;
    match command {
        GetTerminalProfiles(_)
        | GetHomeDirectory(_)
        | DeleteSession(_)
        | AttachTerminal(_)
        | CreateSession(_)
        | AddTab(_)
        | ActivateSession(_) => true,
        ListFiles(v) => v.session_id.is_some() || v.tab_id.is_some(),
        ReadFile(v) => v.session_id.is_some() || v.tab_id.is_some(),
        WriteFile(v) => v.session_id.is_some() || v.tab_id.is_some(),
        ReadFileAsset(v) => v.session_id.is_some() || v.tab_id.is_some(),
        WriteImageFile(v) => v.session_id.is_some() || v.tab_id.is_some(),
        BeginFileRead(v) => v.session_id.is_some() || v.tab_id.is_some(),
        BeginFileWrite(v) => v.session_id.is_some() || v.tab_id.is_some(),
        CreateDirectory(v) => v.session_id.is_some() || v.tab_id.is_some(),
        _ => false,
    }
}
pub(super) fn recollects_when_stale(command: &OwnerCommand) -> bool {
    use wire::RemoteCommand::*;
    matches!(
        command,
        OwnerCommand::Execute {
            command: GetTerminalProfiles(_)
                | GetHomeDirectory(_)
                | ListFiles(_)
                | ReadFile(_)
                | WriteFile(_)
                | ReadFileAsset(_)
                | WriteImageFile(_)
                | BeginFileWrite(_)
                | CreateDirectory(_),
            ..
        }
    )
}
pub(in crate::remote) fn requires_facts(
    command: &OwnerCommand,
    has_runtime: impl FnOnce(wire::TabId) -> bool,
) -> bool {
    if let OwnerCommand::Execute {
        command: wire::RemoteCommand::AttachTerminal(input),
        ..
    } = command
    {
        // Only a cold attach needs to resolve a launch profile. A live attach
        // validates and adopts the runtime on this same owner turn.
        return !has_runtime(input.tab_id);
    }
    if capabilities_only(command) {
        return true;
    }
    matches!(
        command,
        OwnerCommand::Execute {
            command: wire::RemoteCommand::GetSnapshot(_)
                | wire::RemoteCommand::GetSessionStats(_)
                | wire::RemoteCommand::RenameSession(_)
                | wire::RemoteCommand::RenameTab(_)
                | wire::RemoteCommand::ReorderTabs(_)
                | wire::RemoteCommand::SetSessionPinned(_)
                | wire::RemoteCommand::SettleSession(_)
                | wire::RemoteCommand::UnsettleSession(_)
                | wire::RemoteCommand::SetSessionEnv(_)
                | wire::RemoteCommand::CloseTab(_)
                | wire::RemoteCommand::DeleteSession(_),
            ..
        }
    )
}
