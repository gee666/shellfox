use crate::activity::*;
use shellfox_core::TabId;
use shellfox_runtime::{Identity, InspectionPoll, InspectionTicket, ProcessInspection, Runtime};
use shellfox_terminal::inspection::{InspectionPlatform, RawOwnedEvidence};
use std::{collections::HashMap, ffi::OsString};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionScope {
    LocalWindows,
    LocalUnix,
    /// A WSL tab. Its root is the local wsl.exe; guest processes are out of reach.
    Guest,
    Ssh,
    Unsupported,
}
impl InspectionScope {
    /// The inspection sees the shell and its descendants, so status can use it.
    pub(super) fn locally_inspectable(self) -> bool {
        matches!(self, Self::LocalWindows | Self::LocalUnix)
    }
    /// The tab's root process can be inspected. For WSL that proves only the
    /// wsl.exe root, which is enough for automatic expiry, as in 0.9.
    fn root_inspectable(self) -> bool {
        self.locally_inspectable() || self == Self::Guest
    }

    fn matches_platform(self, platform: InspectionPlatform) -> bool {
        matches!(
            (self, platform),
            (
                Self::LocalWindows | Self::Guest,
                InspectionPlatform::Windows
            ) | (Self::LocalUnix, InspectionPlatform::Unix)
        )
    }
}
pub trait RuntimeSource {
    fn runtime(&self, identity: Identity) -> Option<&Runtime>;
}
impl RuntimeSource for HashMap<TabId, Runtime> {
    fn runtime(&self, identity: Identity) -> Option<&Runtime> {
        self.get(&identity.tab_id)
            .filter(|runtime| runtime.identity() == identity)
    }
}
impl RuntimeSource for HashMap<TabId, crate::terminals::OwnedTerminal> {
    fn runtime(&self, identity: Identity) -> Option<&Runtime> {
        self.get(&identity.tab_id)
            .map(|terminal| &terminal.runtime)
            .filter(|runtime| runtime.identity() == identity)
    }
}
struct Entry {
    identity: Identity,
    scope: InspectionScope,
    ticket: Option<(InspectionTicket, u64)>,
    minimum_started_at: u64,
    completed_started_at: Option<u64>,
}
#[derive(Default)]
pub struct RuntimeInspectionState {
    entries: HashMap<TabId, Entry>,
}
impl RuntimeInspectionState {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn watch(
        &mut self,
        identity: Identity,
        scope: InspectionScope,
    ) -> Result<(), InspectionFailure> {
        if let Some(entry) = self.entries.get_mut(&identity.tab_id) {
            if entry.identity == identity {
                entry.scope = scope;
                return Ok(());
            }
        } else if self.entries.len() >= MAX_WATCHES {
            return Err(InspectionFailure::Bounds);
        }
        self.entries.insert(
            identity.tab_id,
            Entry {
                identity,
                scope,
                ticket: None,
                minimum_started_at: 0,
                completed_started_at: None,
            },
        );
        Ok(())
    }
    pub fn forget(&mut self, identity: Identity) {
        if self
            .entries
            .get(&identity.tab_id)
            .is_some_and(|entry| entry.identity == identity)
        {
            self.entries.remove(&identity.tab_id);
        }
    }
    /// Late completion of an older request cannot satisfy an expiry refresh.
    pub fn require_fresh(&mut self, identities: &[Identity], since_ms: u64) {
        for identity in identities.iter().take(MAX_WATCHES) {
            if let Some(entry) = self
                .entries
                .get_mut(&identity.tab_id)
                .filter(|entry| entry.identity == *identity)
            {
                entry.minimum_started_at = entry.minimum_started_at.max(since_ms);
            }
        }
    }
    pub fn fresh_since(&self, identities: &[Identity], since_ms: u64) -> bool {
        identities.len() <= MAX_WATCHES
            && identities.iter().all(|identity| {
                self.entries.get(&identity.tab_id).is_some_and(|entry| {
                    entry.identity == *identity
                        && entry.scope.root_inspectable()
                        && entry.completed_started_at.is_some_and(|started| {
                            started >= since_ms.max(entry.minimum_started_at)
                        })
                })
            })
    }
}
pub struct RuntimeProcessInspector<'a> {
    source: &'a dyn RuntimeSource,
    state: &'a mut RuntimeInspectionState,
    now_ms: u64,
}
impl<'a> RuntimeProcessInspector<'a> {
    pub fn new(
        source: &'a dyn RuntimeSource,
        state: &'a mut RuntimeInspectionState,
        now_ms: u64,
    ) -> Self {
        Self {
            source,
            state,
            now_ms,
        }
    }
}
impl ProcessInspector for RuntimeProcessInspector<'_> {
    fn inspect(&mut self, identity: Identity) -> Result<OwnedProcessSnapshot, InspectionFailure> {
        let entry = self
            .state
            .entries
            .get_mut(&identity.tab_id)
            .filter(|entry| entry.identity == identity)
            .ok_or(InspectionFailure::OwnershipMismatch)?;
        if !entry.scope.root_inspectable() {
            return Err(InspectionFailure::Unavailable);
        }
        let runtime = self
            .source
            .runtime(identity)
            .ok_or(InspectionFailure::OwnershipMismatch)?;
        if let Some((ticket, started)) = entry.ticket {
            match runtime.poll_inspection(ticket) {
                InspectionPoll::Pending => return Err(InspectionFailure::Pending),
                InspectionPoll::Ready(result) => {
                    entry.ticket = None;
                    let result = result.map_err(map_failure)?;
                    if started < entry.minimum_started_at {
                        return Err(InspectionFailure::Pending);
                    }
                    if !entry.scope.matches_platform(result.evidence.platform) {
                        return Err(InspectionFailure::OwnershipMismatch);
                    }
                    let snapshot = convert_inspection(identity, result)?;
                    entry.completed_started_at = Some(started);
                    return Ok(snapshot);
                }
            }
        }
        let ticket = runtime.request_inspection().map_err(map_failure)?;
        entry.ticket = Some((ticket, self.now_ms));
        Err(InspectionFailure::Pending)
    }
}
fn map_failure(error: shellfox_runtime::InspectionFailure) -> InspectionFailure {
    use shellfox_runtime::InspectionFailure as Native;
    match error {
        Native::Pending => InspectionFailure::Pending,
        Native::Bounds => InspectionFailure::Bounds,
        Native::Malformed => InspectionFailure::Malformed,
        Native::OwnershipMismatch => InspectionFailure::OwnershipMismatch,
        Native::Unsupported | Native::Unavailable => InspectionFailure::Unavailable,
    }
}
/// No raw evidence is retained or returned through SourceFacts/remote DTOs.
pub fn convert_inspection(
    identity: Identity,
    result: ProcessInspection,
) -> Result<OwnedProcessSnapshot, InspectionFailure> {
    if result.identity != identity {
        return Err(InspectionFailure::OwnershipMismatch);
    }
    let RawOwnedEvidence {
        platform,
        root,
        root_alive,
        mut complete,
        processes,
    } = result.evidence;
    let (platform, domain) = match platform {
        InspectionPlatform::Windows => (ProcessPlatform::Windows, "windows/local"),
        InspectionPlatform::Unix => (ProcessPlatform::Unix, "unix/local"),
    };
    if root.pid == 0 || root.birth == 0 || processes.len() > MAX_OWNED_PROCESSES {
        return Err(InspectionFailure::Bounds);
    }
    let mut total = 0usize;
    let mut rows = Vec::new();
    let mut roots = 0;
    for process in processes {
        if process.identity.pid == 0 || process.identity.birth == 0 {
            return Err(InspectionFailure::Malformed);
        }
        if process.is_root {
            roots += 1;
            if process.identity != root || roots > 1 {
                return Err(InspectionFailure::OwnershipMismatch);
            }
        }
        let mut exact = true;
        let executable = process
            .executable
            .map(|value| text(value, &mut total, &mut exact))
            .transpose()?;
        let process_name = process
            .process_name
            .map(|value| text(value, &mut total, &mut exact))
            .transpose()?;
        let argv = process
            .argv
            .map(|argv| {
                if argv.len() > 4096 {
                    return Err(InspectionFailure::Bounds);
                }
                argv.into_iter()
                    .map(|value| text(value, &mut total, &mut exact))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        complete &= exact;
        rows.push(OwnedProcess {
            identity: ProcessIdentity {
                domain: domain.into(),
                pid: process.identity.pid,
                birth: process.identity.birth.to_string(),
            },
            is_root: process.is_root,
            accessible: process.accessible && exact,
            executable,
            process_name,
            argv,
        });
    }
    if roots != 1 {
        return Err(InspectionFailure::OwnershipMismatch);
    }
    Ok(OwnedProcessSnapshot {
        identity,
        platform,
        root: if root_alive {
            RootState::Alive
        } else {
            RootState::Exited
        },
        complete,
        processes: rows,
    })
}
fn text(value: OsString, total: &mut usize, exact: &mut bool) -> Result<String, InspectionFailure> {
    if value.len() > 64 * 1024 {
        return Err(InspectionFailure::Bounds);
    }
    let text = match value.into_string() {
        Ok(text) => text,
        Err(value) => {
            *exact = false;
            value.to_string_lossy().into_owned()
        }
    };
    if text.len() > 64 * 1024 || text.contains('\0') {
        return Err(InspectionFailure::Bounds);
    }
    *total = total
        .checked_add(text.len())
        .ok_or(InspectionFailure::Bounds)?;
    if *total > 8 * 1024 * 1024 {
        return Err(InspectionFailure::Bounds);
    }
    Ok(text)
}

#[cfg(test)]
mod scope_tests {
    use super::{InspectionPlatform as P, InspectionScope as S};

    #[test]
    fn a_wsl_root_counts_but_its_status_stays_hidden() {
        assert!(S::Guest.root_inspectable() && !S::Guest.locally_inspectable());
        assert!(S::Guest.matches_platform(P::Windows));
        assert!(!S::Guest.matches_platform(P::Unix));
        assert!(S::LocalUnix.matches_platform(P::Unix));
        assert!(!S::Unsupported.root_inspectable());
    }
}
