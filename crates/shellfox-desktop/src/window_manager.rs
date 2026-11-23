//! Window-local navigation. Snapshot.selection is persisted recent history,
//! not a broadcast instruction to move every window's keyboard destination.
pub mod state;
#[cfg(test)]
mod tests;

use shellfox_core::{Selection, Session, SessionId, Snapshot, TabId, TabState};
use std::collections::VecDeque;

pub const MAX_REMEMBERED_SESSIONS: usize = 128;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialSelection {
    Recent,
    Empty,
    Session(SessionId),
    Tab { session: SessionId, tab: TabId },
}
pub struct WindowSelection {
    initial: Option<InitialSelection>,
    selected: Selection,
    remembered: VecDeque<(SessionId, TabId)>,
    revision: Option<u64>,
    epoch: u64,
}
impl WindowSelection {
    pub fn new(initial: InitialSelection) -> Self {
        Self {
            initial: Some(initial),
            selected: Selection::default(),
            remembered: VecDeque::new(),
            revision: None,
            epoch: 0,
        }
    }
    pub fn selected(&self) -> &Selection {
        &self.selected
    }
    /// Bind delayed close/attach results to this epoch so they cannot move a
    /// newer local selection or focus a different terminal.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn active_tab(&self, session: &Session) -> Option<TabId> {
        self.remembered
            .iter()
            .find(|(id, tab)| *id == session.id && viewable(session, *tab))
            .map(|(_, tab)| *tab)
            .or_else(|| first_tab(session))
    }
    pub fn select_session(&mut self, snapshot: &Snapshot, id: SessionId) -> bool {
        let Some(session) = snapshot.sessions.iter().find(|session| session.id == id) else {
            return false;
        };
        self.set(Selection {
            session_id: Some(id),
            tab_id: self.active_tab(session),
        });
        true
    }
    pub fn select_tab(&mut self, snapshot: &Snapshot, session: SessionId, tab: TabId) -> bool {
        if !snapshot
            .sessions
            .iter()
            .any(|item| item.id == session && viewable(item, tab))
        {
            return false;
        }
        self.set(Selection {
            session_id: Some(session),
            tab_id: Some(tab),
        });
        true
    }
    pub fn clear(&mut self) {
        self.set(Selection::default());
    }
    /// Accept only revisioned snapshots. Read recent selection once at startup;
    /// ordinary changes from another window must not select or focus anything.
    pub fn accept_snapshot(&mut self, snapshot: &Snapshot) -> bool {
        if self
            .revision
            .is_some_and(|revision| snapshot.revision < revision)
        {
            return false;
        }
        self.revision = Some(snapshot.revision);
        self.remembered.retain(|(id, tab)| {
            snapshot
                .sessions
                .iter()
                .any(|session| session.id == *id && viewable(session, *tab))
        });
        if let Some(initial) = self.initial.take() {
            let requested = match initial {
                InitialSelection::Empty => Selection::default(),
                InitialSelection::Recent => snapshot.selection.clone(),
                InitialSelection::Session(id) => Selection {
                    session_id: Some(id),
                    tab_id: None,
                },
                InitialSelection::Tab { session, tab } => Selection {
                    session_id: Some(session),
                    tab_id: Some(tab),
                },
            };
            // A missing explicitly saved tab must never redirect input into a
            // different shell. The same rule applies to stale persisted recent.
            let selection = snapshot
                .sessions
                .iter()
                .find(|session| Some(session.id) == requested.session_id)
                .filter(|session| requested.tab_id.is_none_or(|tab| viewable(session, tab)))
                .map(|session| Selection {
                    session_id: Some(session.id),
                    tab_id: requested.tab_id.or_else(|| first_tab(session)),
                })
                .unwrap_or_default();
            self.set(selection);
        } else if let Some(id) = self.selected.session_id {
            if let Some(session) = snapshot.sessions.iter().find(|session| session.id == id) {
                let tab_id = self
                    .selected
                    .tab_id
                    .filter(|tab| viewable(session, *tab))
                    .or_else(|| self.active_tab(session));
                self.set(Selection {
                    session_id: Some(id),
                    tab_id,
                });
            } else {
                let next = snapshot
                    .sessions
                    .first()
                    .map(|session| Selection {
                        session_id: Some(session.id),
                        tab_id: self.active_tab(session),
                    })
                    .unwrap_or_default();
                self.set(next);
            }
        }
        true
    }
    /// Match the renderer's right-then-left neighbour policy after an explicit
    /// close. A delayed response must supply the epoch from before the request.
    pub fn tab_closed(&mut self, before: &Session, tab: TabId, epoch: u64) -> bool {
        if self.epoch != epoch
            || self.selected.session_id != Some(before.id)
            || self.selected.tab_id != Some(tab)
        {
            return false;
        }
        let tabs: Vec<_> = before
            .tabs
            .iter()
            .filter(|tab| tab.state != TabState::Archived)
            .collect();
        let Some(index) = tabs.iter().position(|item| item.id == tab) else {
            return false;
        };
        let next = tabs
            .get(index + 1)
            .or_else(|| index.checked_sub(1).and_then(|index| tabs.get(index)))
            .map(|tab| tab.id);
        self.set(Selection {
            session_id: Some(before.id),
            tab_id: next,
        });
        true
    }
    fn set(&mut self, selection: Selection) {
        if self.selected != selection {
            self.selected = selection;
            self.epoch = self.epoch.wrapping_add(1);
        }
        if let (Some(session), Some(tab)) = (self.selected.session_id, self.selected.tab_id) {
            self.remembered.retain(|(id, _)| *id != session);
            self.remembered.push_back((session, tab));
            while self.remembered.len() > MAX_REMEMBERED_SESSIONS {
                self.remembered.pop_front();
            }
        }
    }
}
fn viewable(session: &Session, tab: TabId) -> bool {
    session
        .tabs
        .iter()
        .any(|item| item.id == tab && item.state != TabState::Archived)
}
fn first_tab(session: &Session) -> Option<TabId> {
    session
        .tabs
        .iter()
        .find(|tab| tab.state != TabState::Archived)
        .map(|tab| tab.id)
}
