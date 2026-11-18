use crate::{now, storage::Data, *};

impl Host {
    pub fn rename_session(&self, id: SessionId, title: &str) -> Result<Snapshot> {
        validation::title(title)?;
        self.update(ChangeReason::Sessions, |data, _| {
            let session = data.session_mut(id)?;
            if session.title == title {
                return Ok(false);
            }
            session.title = title.into();
            session.updated_at = now();
            Ok(true)
        })
    }
    /// Changes the default for future tabs. Existing processes are not cd'd.
    pub fn set_session_cwd(&self, id: SessionId, cwd: &str) -> Result<Snapshot> {
        validation::cwd(cwd)?;
        self.update(ChangeReason::Sessions, |data, _| {
            let session = data.session_mut(id)?;
            if session.cwd == cwd {
                return Ok(false);
            }
            session.cwd = cwd.into();
            session.updated_at = now();
            Ok(true)
        })
    }
    pub fn set_session_pinned(&self, id: SessionId, pinned: bool) -> Result<Snapshot> {
        self.update(ChangeReason::Sessions, |data, _| {
            let session = data.session_mut(id)?;
            if session.pinned_at.is_some() == pinned {
                return Ok(false);
            }
            session.pinned_at = if pinned { Some(now()) } else { None };
            Ok(true)
        })
    }
    pub fn select_session(&self, id: SessionId) -> Result<Snapshot> {
        self.update(ChangeReason::Sessions, |data, _| {
            let session = data.session(id)?;
            let tab = session
                .tabs
                .iter()
                .find(|t| Some(t.id) == data.state.selection.tab_id)
                .or_else(|| session.tabs.first())
                .map(|t| t.id);
            let changed = data.state.selection
                != Selection {
                    session_id: Some(id),
                    tab_id: tab,
                };
            data.select(id, tab);
            Ok(changed)
        })
    }
    pub fn delete_session(&self, id: SessionId) -> Result<Snapshot> {
        self.update(ChangeReason::Sessions, |data, registry| {
            let session = data.session(id)?;
            if session
                .tabs
                .iter()
                .any(|tab| registry.live.contains_key(&tab.id))
            {
                return Err(Error::new(
                    ErrorCode::CleanupRequired,
                    "Clean up owned terminals before deleting this session.",
                ));
            }
            ensure_not_prepared(
                registry,
                &session.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
                Some(id),
            )?;
            remove_session(data, id);
            Ok(true)
        })
    }
}
pub(crate) fn directory_title(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .filter(|s| s.encode_utf16().count() <= 200)
        .unwrap_or("Session")
        .into()
}
pub(crate) fn remove_session(data: &mut Data, id: SessionId) {
    data.sessions.retain(|s| s.id != id);
    data.creation_facts.remove(&id);
    data.requests.retain(|(_, session)| *session != id);
    data.repair_selection();
}
pub(crate) fn ensure_not_prepared(
    registry: &Registry,
    tabs: &[TabId],
    session: Option<SessionId>,
) -> Result<()> {
    if registry.pending.values().any(|p| match p.target {
        CloseTarget::Tab(id) => tabs.contains(&id),
        CloseTarget::Session(id) => Some(id) == session,
    }) {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Cleanup is already prepared.",
        ));
    }
    Ok(())
}
