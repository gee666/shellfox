use super::*;
use std::collections::HashSet;
pub(super) fn legacy_session(mut session: wire::SessionDto) -> wire::SessionDto {
    session.can_focus = false; session.can_add_tab = false;
    if session.control_reason.is_none() { session.control_reason = Some("Legacy full-state terminal recovery is unavailable on this Rust host.".into()); }
    session
}
impl RemoteService<'_> {
    /// Existing manager policy hides explicitly local-only TAB records, not the
    /// saved session or authorized unknown metadata. This is not a live grant.
    pub(super) fn private_tabs(&self, snapshot: &Snapshot) -> HashSet<shellfox_core::TabId> {
        snapshot.sessions.iter().flat_map(|session| session.tabs.iter()).filter(|tab| {
            self.state.profiles.as_ref().is_some_and(|profiles| profiles.profiles.iter().any(|profile|
                profile.id == tab.profile_id.as_str() && profile.elevation.is_some()))
            || self.service.terminals.get(&tab.id).is_some_and(|terminal| terminal.runtime.identity().session_id == tab.session_id
                && terminal.runtime.launch_privilege_facts().privilege == shellfox_runtime::LaunchPrivilege::Elevated)
        }).map(|tab| tab.id).collect()
    }
    pub(super) fn authorize_profiles(&self, command: &RemoteCommand) -> Result<(), AppError> {
        use RemoteCommand::*;
        let (session_id, tab_id, source_operation) = match command {
            RenameSession(v) => (Some(v.session_id),None,false), RenameTab(v) => (Some(v.session_id),Some(v.tab_id),false),
            ReorderTabs(v) => (Some(v.session_id),None,false), SetSessionPinned(v) => (Some(v.session_id),None,false),
            SettleSession(v) => (Some(v.session_id),None,false), UnsettleSession(v) => (Some(v.session_id),None,false),
            ClearSessionError(v) => (Some(v.session_id),None,false), SetSessionEnv(v) => (Some(v.session_id),None,false),
            DeleteSession(v) => (Some(v.session_id),None,true), CloseTab(v) => (None,Some(v.tab_id),true),
            ListFiles(v) => (v.session_id,v.tab_id,true), ReadFile(v) => (v.session_id,v.tab_id,true),
            WriteFile(v) => (v.session_id,v.tab_id,true), ReadFileAsset(v) => (v.session_id,v.tab_id,true),
            WriteImageFile(v) => (v.session_id,v.tab_id,true), BeginFileRead(v) => (v.session_id,v.tab_id,true),
            BeginFileWrite(v) => (v.session_id,v.tab_id,true), CreateDirectory(v) => (v.session_id,v.tab_id,true),
            _ => return Ok(()),
        };
        if session_id.is_none() && tab_id.is_none() { return Ok(()); }
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        let private = self.private_tabs(&snapshot);
        let session = snapshot.sessions.iter().find(|session| session_id.is_none_or(|id| session.id == id)
            && tab_id.is_none_or(|id| session.tabs.iter().any(|tab| tab.id == id)))
            .ok_or_else(|| failure(ErrorCode::NotFound, "Remote target not found."))?;
        if source_operation && (tab_id.is_some_and(|id| private.contains(&id))
            || matches!(command, DeleteSession(_)) && session.tabs.iter().any(|tab| private.contains(&tab.id))) {
            return Err(failure(ErrorCode::AccessDenied, "Elevated sources remain local-only."));
        }
        Ok(())
    }
}
