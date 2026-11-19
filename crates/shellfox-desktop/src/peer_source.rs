//! Window selection/cache identity. A peer catalog is never a local Snapshot.
use anyhow::{Result, anyhow};
use shellfox_protocol::{AuthenticatedPeerId, Validate, manager_v2 as m};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Local,
    Peer(AuthenticatedPeerId),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PeerCycle {
    pub peer: AuthenticatedPeerId,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Disconnected,
    Connecting,
    AwaitingApproval,
    Connected,
    Rejected,
    Reconnecting,
}
#[derive(Clone)]
pub(crate) struct PeerState {
    pub hostname: String,
    pub online: bool,
    pub remembered: bool,
    pub home: Option<String>,
    pub phase: Phase,
    pub cycle: u64,
    pub snapshot: Option<m::Snapshot>,
    catalog_cycle: Option<u64>,
    pub origin: Option<shellfox_client::manager_v2::ManagerBindingWitness>,
    pub selected_session: Option<m::Id>,
    pub selected_tab: Option<m::Id>,
    pub selection_committed: bool,
    pub pending: bool,
    pub error: Option<String>,
}
pub(crate) struct PeerSources {
    pub selected: Source,
    pub peers: HashMap<AuthenticatedPeerId, PeerState>,
    pub inventory_pending: bool,
    selection_epoch: u64,
}
impl Default for PeerSources {
    fn default() -> Self {
        Self {
            selected: Source::Local,
            peers: HashMap::new(),
            inventory_pending: false,
            selection_epoch: 1,
        }
    }
}
impl PeerSources {
    /// Presentation cancellation only, never a Manager binding/file grant.
    pub fn selection_epoch(&self) -> u64 {
        self.selection_epoch
    }
    pub fn choose(&mut self, source: Source) -> Result<()> {
        if self.selected != source {
            let next = self
                .selection_epoch
                .checked_add(1)
                .ok_or_else(|| anyhow!("Source selection epoch exhausted."))?;
            self.selected = source;
            self.selection_epoch = next;
        }
        Ok(())
    }
    pub fn inventory(&mut self, peers: &[shellfox_service::remote::native::RemotePeer]) {
        self.peers.retain(|id, state| {
            peers.iter().any(|peer| peer.id == id.as_str())
                || !matches!(state.phase, Phase::Disconnected | Phase::Rejected)
                || self.selected == Source::Peer(id.clone())
        });
        for peer in peers.iter().take(1000) {
            let Ok(id) = AuthenticatedPeerId::new(peer.id.clone()) else {
                continue;
            };
            if !self.peers.contains_key(&id) && self.peers.len() >= 1000 {
                continue;
            }
            let state = self.peers.entry(id).or_insert_with(|| PeerState {
                hostname: peer.hostname.clone(),
                online: false,
                remembered: false,
                home: None,
                phase: Phase::Disconnected,
                cycle: 0,
                snapshot: None,
                catalog_cycle: None,
                origin: None,
                selected_session: None,
                selected_tab: None,
                selection_committed: false,
                pending: false,
                error: None,
            });
            state.hostname = peer.hostname.clone();
            state.online = peer.online;
            state.remembered = peer.remembered;
            if state.cycle == 0 {
                // A Settings operation can reach these states before this window
                // has a terminal catalog or has selected the peer.
                use shellfox_service::remote::native::RemoteConnection as Network;
                state.phase = match peer.connection {
                    Network::Connecting => Phase::Connecting,
                    Network::Reconnecting => Phase::Reconnecting,
                    Network::AwaitingApproval => Phase::AwaitingApproval,
                    Network::Rejected => Phase::Rejected,
                    Network::Disconnected => Phase::Disconnected,
                    Network::Connected => state.phase,
                };
                state.error = peer.last_error.as_ref().map(|error| error.message.clone());
            }
        }
        for (id, state) in &mut self.peers {
            if !peers.iter().any(|peer| peer.id == id.as_str()) {
                state.online = false;
            }
        }
    }
    pub fn begin(&mut self, peer: &AuthenticatedPeerId) -> anyhow::Result<PeerCycle> {
        let state = self.peers.get_mut(peer).ok_or_else(|| {
            anyhow::anyhow!("Peer is not in the discovered/remembered host catalog.")
        })?;
        anyhow::ensure!(!state.pending, "A peer operation is already pending.");
        state.cycle = state
            .cycle
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Peer cycle exhausted."))?;
        state.phase = match state.phase {
            Phase::AwaitingApproval => Phase::AwaitingApproval,
            _ if state.snapshot.is_some() => Phase::Reconnecting,
            _ => Phase::Connecting,
        };
        state.pending = true;
        state.error = None;
        state.origin = None;
        Ok(PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        })
    }
    pub fn current(&self, cycle: &PeerCycle) -> bool {
        self.peers
            .get(&cycle.peer)
            .is_some_and(|state| state.cycle == cycle.generation)
    }
    pub fn phase(&mut self, cycle: &PeerCycle, phase: Phase, error: Option<String>) -> bool {
        let Some(state) = self
            .peers
            .get_mut(&cycle.peer)
            .filter(|state| state.cycle == cycle.generation)
        else {
            return false;
        };
        state.phase = phase;
        state.error = error;
        state.pending = false;
        if phase != Phase::Connected {
            state.origin = None;
        }
        if matches!(phase, Phase::Disconnected | Phase::Rejected) {
            state.snapshot = None;
            state.catalog_cycle = None;
        }
        true
    }
    pub fn catalog(&mut self, cycle: &PeerCycle, snapshot: m::Snapshot) -> anyhow::Result<bool> {
        snapshot.validate()?;
        let Some(state) = self
            .peers
            .get_mut(&cycle.peer)
            .filter(|state| state.cycle == cycle.generation && state.phase == Phase::Connected)
        else {
            return Ok(false);
        };
        if state.catalog_cycle == Some(cycle.generation)
            && state
                .snapshot
                .as_ref()
                .is_some_and(|old| old.revision > snapshot.revision)
        {
            return Ok(false);
        }
        let session = state
            .selected_session
            .and_then(|id| sessions(&snapshot).find(|session| session.id == id));
        let restoring = state.catalog_cycle != Some(cycle.generation);
        let valid_tab = state.selected_tab.is_none_or(|id| {
            session.is_some_and(|session| {
                session
                    .tabs
                    .iter()
                    .any(|tab| tab.id == id && (!restoring || viewable_tab(tab)))
            })
        });
        if session.is_none() || restoring && !valid_tab {
            state.selected_session = None;
            state.selected_tab = None;
        } else if !valid_tab {
            state.selected_tab = None;
        } else if restoring && state.selected_tab.is_none() {
            state.selected_tab = session
                .and_then(|session| session.tabs.iter().find(|tab| viewable_tab(tab)))
                .map(|tab| tab.id);
        }
        state.snapshot = Some(snapshot);
        state.catalog_cycle = Some(cycle.generation);
        Ok(true)
    }
    pub fn select(
        &mut self,
        cycle: &PeerCycle,
        session: m::Id,
        tab: Option<m::Id>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.current(cycle),
            "Peer selection belongs to a retired connection."
        );
        let state = self
            .peers
            .get_mut(&cycle.peer)
            .ok_or_else(|| anyhow::anyhow!("Unknown peer."))?;
        anyhow::ensure!(state.phase == Phase::Connected, "Peer is not connected.");
        let snapshot = state
            .snapshot
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Peer catalog is not available."))?;
        let selected = sessions(snapshot)
            .find(|item| item.id == session)
            .ok_or_else(|| anyhow::anyhow!("Peer session is unavailable."))?;
        anyhow::ensure!(
            tab.is_none_or(|id| selected.tabs.iter().any(|item| item.id == id)),
            "Peer tab does not belong to this session."
        );
        let existing = if state.selected_session == Some(session) {
            state.selected_tab
        } else {
            None
        };
        let viewable = |tab: &&m::Tab| viewable_tab(tab);
        state.selection_committed = true;
        state.selected_session = Some(session);
        state.selected_tab = tab.or_else(|| {
            existing
                .filter(|id| {
                    selected
                        .tabs
                        .iter()
                        .filter(viewable)
                        .any(|tab| tab.id == *id)
                })
                .or_else(|| selected.tabs.iter().find(viewable).map(|tab| tab.id))
        });
        self.choose(Source::Peer(cycle.peer.clone()))?;
        Ok(())
    }
    pub fn selected_peer(&self) -> Option<(&AuthenticatedPeerId, &PeerState)> {
        match &self.selected {
            Source::Local => None,
            Source::Peer(peer) => self.peers.get(peer).map(|state| (peer, state)),
        }
    }
    pub fn retire(&mut self, peer: &AuthenticatedPeerId) -> anyhow::Result<()> {
        if let Some(state) = self.peers.get_mut(peer) {
            state.cycle = state
                .cycle
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Peer cycle exhausted."))?;
            state.phase = Phase::Disconnected;
            state.pending = false;
            state.snapshot = None;
            state.catalog_cycle = None;
            state.origin = None;
            // Selection IDs are UI preferences, never a retained binding.
        }
        Ok(())
    }
}
pub(crate) fn viewable_tab(tab: &m::Tab) -> bool {
    if matches!(tab.facts.terminal_kind.known(), Some(Some(shellfox_protocol::TerminalKind::ExternalLegacy))) {
        return false;
    }
    !matches!(tab.facts.lifecycle.known(), Some(shellfox_protocol::TabLifecycle::Closed))
        || tab.facts.profile_id.known().and_then(Option::as_deref) == Some(shellfox_service::AUTOMATION_PROFILE_ID)
}

pub(crate) fn sessions(snapshot: &m::Snapshot) -> impl Iterator<Item = &m::Session> {
    snapshot.entries.iter().filter_map(|entry| match entry {
        m::CatalogEntry::Session { value } => Some(value.as_ref()),
        m::CatalogEntry::Unavailable { .. } => None,
    })
}
#[cfg(test)]
#[path = "peer_source_tests.rs"]
mod tests;
