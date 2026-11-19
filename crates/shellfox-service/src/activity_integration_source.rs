use crate::activity::Observation;
use shellfox_core::{Settings, Snapshot};
use shellfox_protocol::CancellationToken;
use shellfox_runtime::{Identity, Phase};
use std::{collections::HashSet, sync::Arc};
#[cfg(test)]
#[path = "activity_integration_source_tests.rs"]
mod tests;

#[derive(Clone)]
pub struct TerminalSourceFacts {
    pub identity: Identity,
    pub phase: Phase,
    /// Actual registered Runtime kind. Policy-only captures have no such proof.
    pub source_kind: Option<shellfox_runtime::SourceKind>,
    /// A completed published automation replay with sealed normal launch evidence.
    pub automation_exit_code: Option<i64>,
    pub startup: super::StartupEvidence,
    pub observation: Option<Observation>,
    pub busy: bool,
    pub activity_available: bool,
}
/// Immutable owner evidence for asynchronous providers. No raw native identities,
/// process paths/arguments, Runtime references, Host connection or execution rights.
/// Saved environment settings remain internal until the authorized DTO projection.
#[derive(Clone)]
pub struct SourceFacts {
    pub sequence: u64,
    pub snapshot: Snapshot,
    pub settings: Settings,
    pub terminals: Vec<TerminalSourceFacts>,
    pub creation_facts: shellfox_host::SessionCreationFactMap,
    #[cfg(windows)]
    pub(crate) wsl_profiles: Option<Arc<crate::windows_profiles::WslProfiles>>,
    pub cancelled: CancellationToken,
}
pub type SourceFactsWatch = tokio::sync::watch::Receiver<Option<Arc<SourceFacts>>>;
pub struct SourceFactsPublisher {
    sender: tokio::sync::watch::Sender<Option<Arc<SourceFacts>>>,
    sequence: u64,
    #[cfg(windows)]
    wsl_profiles: Option<Arc<crate::windows_profiles::WslProfiles>>,
}
impl SourceFactsPublisher {
    pub fn new() -> (Self, SourceFactsWatch) {
        let (sender, receiver) = tokio::sync::watch::channel(None);
        (
            Self {
                sender,
                sequence: 0,
                #[cfg(windows)]
                wsl_profiles: None,
            },
            receiver,
        )
    }
    #[cfg(windows)]
    pub(crate) fn set_wsl_profiles(&mut self, profiles: Option<Arc<crate::windows_profiles::WslProfiles>>) {
        self.wsl_profiles = profiles;
    }

    /// Only the service owner captures snapshot/settings, after ownership checks.
    /// A bad or cancelled capture invalidates old evidence instead of publishing it.
    pub fn publish(
        &mut self,
        snapshot: Snapshot,
        settings: Settings,
        terminals: Vec<TerminalSourceFacts>,
        cancelled: CancellationToken,
    ) -> Result<(), crate::activity::InspectionFailure> {
        // Explicitly fact-less callers remain unknown, never backfilled.
        self.publish_with_creation_facts(
            snapshot,
            settings,
            terminals,
            Default::default(),
            cancelled,
        )
    }
    pub fn publish_with_creation_facts(
        &mut self,
        snapshot: Snapshot,
        settings: Settings,
        mut terminals: Vec<TerminalSourceFacts>,
        creation_facts: shellfox_host::SessionCreationFactMap,
        cancelled: CancellationToken,
    ) -> Result<(), crate::activity::InspectionFailure> {
        let valid = snapshot.sessions.len() <= 10_000
            && terminals.len() <= crate::activity::MAX_WATCHES
            && terminals
                .iter()
                .all(|terminal| terminal.startup.validate().is_ok())
            && !cancelled.is_cancelled()
            && settings.validate().is_ok()
            && snapshot.preferences.validate().is_ok()
            && snapshot
                .sessions
                .iter()
                .all(|session| session.validate().is_ok());
        let mut seen = HashSet::new();
        let members = terminals.iter().all(|terminal| {
            seen.insert(terminal.identity.tab_id)
                && snapshot.sessions.iter().any(|session| {
                    session.id == terminal.identity.session_id
                        && session
                            .tabs
                            .iter()
                            .any(|tab| tab.id == terminal.identity.tab_id)
                })
        });
        let creation_valid = creation_facts.len() <= snapshot.sessions.len()
            && creation_facts.iter().all(|(id, fact)| {
                fact.validate().is_ok() && snapshot.sessions.iter().any(|session| session.id == *id)
            });
        let Some(sequence) = self
            .sequence
            .checked_add(1)
            .filter(|_| valid && members && creation_valid)
        else {
            self.invalidate();
            return Err(crate::activity::InspectionFailure::Malformed);
        };
        terminals.sort_by_key(|terminal| terminal.identity.tab_id);
        self.sequence = sequence;
        self.sender.send_replace(Some(Arc::new(SourceFacts {
            sequence,
            snapshot,
            settings,
            terminals,
            creation_facts,
            #[cfg(windows)]
            wsl_profiles: self.wsl_profiles.clone(),
            cancelled,
        })));
        Ok(())
    }
    pub fn invalidate(&self) {
        self.sender.send_replace(None);
    }
}
impl Drop for SourceFactsPublisher {
    fn drop(&mut self) {
        self.invalidate();
    }
}
/// Clone and release the watch borrow before awaiting discovery or permissions.
pub fn read_source(watch: &SourceFactsWatch) -> Option<Arc<SourceFacts>> {
    watch
        .borrow()
        .clone()
        .filter(|source| !source.cancelled.is_cancelled())
}
pub fn source_is_current(watch: &SourceFactsWatch, captured: &SourceFacts) -> bool {
    read_source(watch).is_some_and(|current| {
        current.sequence == captured.sequence && std::ptr::eq(current.as_ref(), captured)
    }) && !captured.cancelled.is_cancelled()
}
impl SourceFacts {
    /// The provider must also recheck source_is_current after asynchronous work.
    pub fn matches_request(&self, request: &crate::remote::ManagerFactsRequest) -> bool {
        if self.cancelled.is_cancelled()
            || request.cancelled.is_cancelled()
            || self.snapshot != request.snapshot
            || self.settings != request.settings
            || self.terminals.len() != request.terminals.len()
        {
            return false;
        }
        let mut seen = HashSet::new();
        request.terminals.iter().all(|requested| {
            seen.insert(requested.identity.tab_id)
                && self.terminals.iter().any(|actual| {
                    actual.identity == requested.identity && actual.phase == requested.phase
                })
        })
    }
}
