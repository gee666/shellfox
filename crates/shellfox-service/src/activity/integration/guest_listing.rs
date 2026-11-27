//! Lists WSL guest processes off the owner thread. One listing at a time per
//! distribution and account serves all of its tabs; the owner only polls.
use crate::activity::{GuestEvidence, OwnedProcessSnapshot, PROCESS_POLL_MS};
use shellfox_terminal::wsl::{GuestProcesses, WslProfileId};
use std::{
    collections::HashMap,
    sync::{mpsc, Arc},
};

/// A listing this old no longer describes the tab.
const MAX_AGE_MS: u64 = 20_000;
const MAX_TARGETS: usize = 32;

/// Where a tab's guest processes may run.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GuestTarget {
    /// A WSL tab: its distribution and launch account.
    Tab {
        distro: WslProfileId,
        account: String,
    },
    /// `wsl` started inside a Windows tab inherits its marker through
    /// WSLENV, in whichever distribution it opened. As in 0.9, every running
    /// distribution is listed as its default user.
    Running,
}
impl GuestTarget {
    /// Running distributions, for a Windows tab whose process tree holds a
    /// `wsl.exe` besides its root. A WSL tab's own wsl.exe is its root.
    pub(super) fn nested(snapshot: &OwnedProcessSnapshot) -> Option<Self> {
        snapshot
            .processes
            .iter()
            .any(|process| {
                !process.is_root
                    && process.executable.as_deref().is_some_and(|path| {
                        crate::activity::matching::basename(path, true)
                            .eq_ignore_ascii_case("wsl.exe")
                    })
            })
            .then_some(Self::Running)
    }
}

type GuestLister = dyn Fn(&GuestTarget) -> Option<GuestProcesses> + Send + Sync;

#[derive(Default)]
struct Slot {
    pending: Option<(u64, mpsc::Receiver<Option<GuestProcesses>>)>,
    latest: Option<(u64, Option<Arc<GuestProcesses>>)>,
}

pub struct GuestListings {
    lister: Arc<GuestLister>,
    slots: HashMap<GuestTarget, Slot>,
}
impl Default for GuestListings {
    fn default() -> Self {
        Self {
            lister: Arc::new(os_listing),
            slots: HashMap::new(),
        }
    }
}
impl GuestListings {
    #[cfg(test)]
    pub(super) fn with_lister(lister: Arc<GuestLister>) -> Self {
        Self {
            lister,
            slots: HashMap::new(),
        }
    }
    /// The latest listing started at or after `since_ms`, for the tab
    /// `marker`. Starts the next listing once the latest is a poll old.
    pub fn evidence(
        &mut self,
        target: &GuestTarget,
        marker: String,
        since_ms: u64,
        now_ms: u64,
    ) -> GuestEvidence {
        if !self.slots.contains_key(target) && self.slots.len() >= MAX_TARGETS {
            return GuestEvidence::Unavailable;
        }
        let slot = self.slots.entry(target.clone()).or_default();
        if let Some((started, receiver)) = &slot.pending {
            let finished = match receiver.try_recv() {
                Ok(listing) => Some(listing.map(Arc::new)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(None),
            };
            if let Some(listing) = finished {
                slot.latest = Some((*started, listing));
                slot.pending = None;
            }
        }
        let due = slot
            .latest
            .as_ref()
            .is_none_or(|(started, _)| now_ms >= started.saturating_add(PROCESS_POLL_MS));
        if slot.pending.is_none() && due {
            let (sender, receiver) = mpsc::sync_channel(1);
            let (lister, target) = (Arc::clone(&self.lister), target.clone());
            let spawned = std::thread::Builder::new()
                .name("wsl-guest-processes".into())
                .spawn(move || {
                    let _ = sender.send(lister(&target));
                });
            match spawned {
                Ok(_) => slot.pending = Some((now_ms, receiver)),
                Err(_) => slot.latest = Some((now_ms, None)),
            }
        }
        match &slot.latest {
            Some((started, Some(listing)))
                if *started >= since_ms && now_ms.saturating_sub(*started) <= MAX_AGE_MS =>
            {
                GuestEvidence::Listed {
                    listing: Arc::clone(listing),
                    marker,
                }
            }
            _ => GuestEvidence::Unavailable,
        }
    }

    /// Drops targets no tab uses. A running listing finishes on its own.
    pub fn retain(&mut self, used: impl Fn(&GuestTarget) -> bool) {
        self.slots.retain(|target, _| used(target));
    }
}

fn os_listing(target: &GuestTarget) -> Option<GuestProcesses> {
    #[cfg(windows)]
    {
        let wsl = shellfox_terminal::wsl::OsWsl::discover().ok()?;
        match target {
            GuestTarget::Tab { distro, account } => wsl.guest_processes(distro, Some(account)).ok(),
            GuestTarget::Running => {
                let distros = wsl.running().ok()?;
                // In parallel, so one slow guest doesn't delay the others.
                let listings: Vec<_> = std::thread::scope(|scope| {
                    let wsl = &wsl;
                    let threads: Vec<_> = distros
                        .iter()
                        .map(|distro| scope.spawn(move || wsl.guest_processes(distro, None)))
                        .collect();
                    threads
                        .into_iter()
                        .map(|thread| thread.join().ok())
                        .collect()
                });
                Some(merge(listings.into_iter().map(|listing| listing?.ok())))
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        None
    }
}

/// One listing for several distributions. A distribution that could not be
/// listed leaves it incomplete, so a marked process may be missing.
#[cfg(any(windows, test))]
fn merge(listings: impl Iterator<Item = Option<GuestProcesses>>) -> GuestProcesses {
    let mut merged = GuestProcesses {
        processes: Vec::new(),
        complete: true,
    };
    for listing in listings {
        match listing {
            Some(listing) => {
                merged.complete &= listing.complete;
                merged.processes.extend(listing.processes);
            }
            None => merged.complete = false,
        }
    }
    merged
}

#[cfg(test)]
mod tests;
#[cfg(all(test, windows))]
mod wsl_tests;
