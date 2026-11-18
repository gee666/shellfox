use crate::{now, sessions::ensure_not_prepared, storage::Data, *};
use std::collections::HashSet;
impl Host {
    pub fn add_tab(&self, id: SessionId, input: AddTab) -> Result<Snapshot> {
        if let Some(title) = &input.title {
            validation::title(title)?;
        }
        if let Some(cwd) = &input.cwd {
            validation::cwd(cwd)?;
        }
        self.update(ChangeReason::Sessions, move |data, registry| {
            ensure_not_prepared(registry, &[], Some(id))?;
            let session = data.session_mut(id)?;
            let tab_id = TabId::new();
            let ordinal = u32::try_from(session.tabs.len())
                .map_err(|_| Error::new(ErrorCode::Validation, "Too many tabs."))?;
            let created_at = now();
            session.tabs.push(Tab {
                id: tab_id,
                session_id: id,
                title: input.title.unwrap_or_else(|| input.profile_id.to_string()),
                cwd: input.cwd.unwrap_or_else(|| session.cwd.clone()),
                ordinal,
                created_at: Some(created_at.clone()),
                profile_id: input.profile_id,
                state: TabState::Pending,
            });
            session.updated_at = created_at;
            session.settled_at = None;
            data.select(id, Some(tab_id));
            Ok(true)
        })
    }
    pub fn rename_tab(&self, id: TabId, title: &str) -> Result<Snapshot> {
        validation::title(title)?;
        self.update(ChangeReason::Sessions, |data, _| {
            let tab = data.tab_mut(id)?;
            if tab.title == title {
                return Ok(false);
            }
            tab.title = title.into();
            Ok(true)
        })
    }
    /// Editing stored launch intent never changes a live shell's directory.
    pub fn set_tab_cwd(&self, id: TabId, cwd: &str) -> Result<Snapshot> {
        validation::cwd(cwd)?;
        self.update(ChangeReason::Sessions, |data, registry| {
            if registry.live.contains_key(&id) {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Cannot edit a live terminal launch directory.",
                ));
            }
            let tab = data.tab_mut(id)?;
            if tab.cwd == cwd {
                return Ok(false);
            }
            tab.cwd = cwd.into();
            Ok(true)
        })
    }
    pub fn reorder_tabs(&self, id: SessionId, tab_ids: &[TabId]) -> Result<Snapshot> {
        self.update(ChangeReason::Sessions, |data, _| {
            let session = data.session_mut(id)?;
            let wanted: HashSet<_> = tab_ids.iter().copied().collect();
            validation::ensure(
                tab_ids.len() == session.tabs.len()
                    && wanted.len() == tab_ids.len()
                    && session.tabs.iter().all(|t| wanted.contains(&t.id)),
                "Expected a complete tab permutation.",
            )?;
            if session
                .tabs
                .iter()
                .map(|t| t.id)
                .eq(tab_ids.iter().copied())
            {
                return Ok(false);
            }
            session
                .tabs
                .sort_by_key(|t| tab_ids.iter().position(|id| *id == t.id));
            for (ordinal, tab) in session.tabs.iter_mut().enumerate() {
                tab.ordinal = ordinal as u32;
            }
            Ok(true)
        })
    }
    pub fn select_tab(&self, id: TabId) -> Result<Snapshot> {
        self.update(ChangeReason::Sessions, |data, _| {
            let tab = data.tab(id)?;
            let selection = Selection {
                session_id: Some(tab.session_id),
                tab_id: Some(id),
            };
            let changed = data.state.selection != selection;
            data.state.selection = selection;
            Ok(changed)
        })
    }
    pub fn close_tab(&self, id: TabId) -> Result<Snapshot> {
        self.update(ChangeReason::Sessions, |data, registry| {
            let parent = data.tab(id)?.session_id;
            if registry.live.contains_key(&id) {
                return Err(Error::new(
                    ErrorCode::CleanupRequired,
                    "Clean up the owned terminal before closing this tab.",
                ));
            }
            ensure_not_prepared(registry, &[id], Some(parent))?;
            remove_tab(data, id)?;
            Ok(true)
        })
    }
}
pub(crate) fn remove_tab(data: &mut Data, id: TabId) -> Result<()> {
    let parent = data.tab(id)?.session_id;
    let session = data.session_mut(parent)?;
    session.tabs.retain(|t| t.id != id);
    for (ordinal, tab) in session.tabs.iter_mut().enumerate() {
        tab.ordinal = ordinal as u32;
    }
    session.updated_at = now();
    data.repair_selection();
    Ok(())
}
