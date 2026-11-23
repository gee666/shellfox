//! `session stats` and `session processes`, local or from a peer: one
//! on-demand capture of each live tab's owned processes, off the owner thread. Nothing here runs periodically.
//! The capture is the read-only activity inspection: only processes inside the
//! tab's own Job, cgroup or session, run by this user.
use crate::Service;
use shellfox_protocol::{ProcessStat, TabStat};
use shellfox_terminal::inspection::{InspectionCapability, InspectionPlatform, RawOwnedEvidence};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::oneshot;

/// Captures in flight at once; more are refused, never queued.
const MAX_CAPTURES: usize = 2;
static CAPTURES: AtomicUsize = AtomicUsize::new(0);
/// The reply bound of `getSessionStats`.
const MAX_TABS: usize = 64;

pub(crate) enum StatsReply {
    Native(oneshot::Sender<anyhow::Result<crate::native::NativeResponse>>),
    Desktop(oneshot::Sender<anyhow::Result<serde_json::Value>>),
    /// A peer's `getSessionStats`; the owner finishes the request on its next pass.
    Remote(oneshot::Sender<anyhow::Result<Vec<TabStat>>>),
}
impl StatsReply {
    fn send(self, result: anyhow::Result<Vec<TabStat>>) {
        match self {
            Self::Native(reply) => {
                let _ = reply.send(result.map(crate::native::NativeResponse::TabStats));
            }
            Self::Desktop(reply) => {
                let value = result.and_then(|stats| {
                    Ok(serde_json::to_value(crate::local::DesktopReply::TabStats(
                        stats,
                    ))?)
                });
                let _ = reply.send(value);
            }
            Self::Remote(reply) => {
                let _ = reply.send(result);
            }
        }
    }
}

struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        CAPTURES.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Service {
    pub(crate) fn enqueue_tab_stats(&mut self, reply: StatsReply) {
        if let Err(error) = self.assert_accepting() {
            return reply.send(Err(error));
        }
        if CAPTURES.fetch_add(1, Ordering::AcqRel) >= MAX_CAPTURES {
            CAPTURES.fetch_sub(1, Ordering::AcqRel);
            return reply.send(Err(anyhow::anyhow!(
                "Session stats are busy; retry shortly."
            )));
        }
        let slot = Slot;
        let tabs: Vec<_> = self
            .terminals
            .iter()
            .take(MAX_TABS)
            .map(|(tab, terminal)| (*tab, terminal.runtime.inspection_capability()))
            .collect();
        let wake = self.wake_on_exit();
        let spawned = std::thread::Builder::new()
            .name("shellfox-tab-stats".into())
            .spawn(move || {
                // Dropped after the reply, so the owner sees it on its next pass.
                let _wake = wake;
                let _slot = slot;
                let now = now_ms();
                let stats = tabs
                    .into_iter()
                    .map(|(tab, capability)| measure(tab, capability, now))
                    .collect();
                reply.send(Ok(stats));
            });
        if let Err(error) = spawned {
            // The closure, its slot and the reply were dropped with the failed spawn.
            eprintln!("Shellfox host: could not start a stats capture: {error}");
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
fn unavailable(tab: shellfox_core::TabId, reason: &str) -> TabStat {
    TabStat {
        tab_id: tab,
        processes: Vec::new(),
        memory_bytes: None,
        elapsed_seconds: None,
        reason: Some(reason.into()),
    }
}
fn measure(
    tab: shellfox_core::TabId,
    capability: Option<InspectionCapability>,
    now: u64,
) -> TabStat {
    let Some(capability) = capability else {
        return unavailable(
            tab,
            "Process measurements are unavailable for this terminal.",
        );
    };
    match capability.capture() {
        Ok(evidence) => stats(tab, evidence, now),
        Err(error) => unavailable(tab, &error.to_string()),
    }
}

/// 0.9's shape: per-process rows, tab memory as their sum, duration of the shell.
pub(crate) fn stats(tab: shellfox_core::TabId, evidence: RawOwnedEvidence, now: u64) -> TabStat {
    let domain = match evidence.platform {
        InspectionPlatform::Windows => "windows/local",
        InspectionPlatform::Unix => "unix/local",
    };
    let elapsed = |started: Option<u64>| started.map(|s| now.saturating_sub(s) as f64 / 1000.0);
    let processes: Vec<ProcessStat> = evidence
        .processes
        .iter()
        .map(|p| ProcessStat {
            pid: p.identity.pid,
            parent_pid: p.parent_pid.unwrap_or(0),
            name: p
                .process_name
                .as_ref()
                .map(|name| name.to_string_lossy().chars().take(256).collect())
                .unwrap_or_default(),
            memory_bytes: p.memory_bytes,
            elapsed_seconds: elapsed(p.started_ms),
            domain: domain.into(),
        })
        .collect();
    let root = evidence.processes.iter().find(|p| p.is_root);
    let memory = processes
        .iter()
        .map(|p| p.memory_bytes)
        .sum::<Option<u64>>();
    let reason =
        (!evidence.complete).then(|| "Some owned processes could not be measured.".to_owned());
    TabStat {
        tab_id: tab,
        memory_bytes: memory.filter(|_| !processes.is_empty()),
        elapsed_seconds: root.and_then(|root| elapsed(root.started_ms)),
        processes,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_terminal::inspection::{RawProcessEvidence, RawProcessIdentity};

    fn row(pid: u32, parent: u32, root: bool, memory: Option<u64>) -> RawProcessEvidence {
        RawProcessEvidence {
            identity: RawProcessIdentity { pid, birth: 1 },
            is_root: root,
            accessible: true,
            executable: None,
            process_name: Some(format!("p{pid}").into()),
            argv: None,
            parent_pid: Some(parent),
            memory_bytes: memory,
            started_ms: Some(1_000),
        }
    }
    #[test]
    fn tab_memory_sums_its_processes_and_duration_is_the_shells() {
        let tab = shellfox_core::TabId::new();
        let evidence = |processes| RawOwnedEvidence {
            platform: InspectionPlatform::Unix,
            root: RawProcessIdentity { pid: 7, birth: 1 },
            root_alive: true,
            complete: true,
            processes,
        };
        let stat = stats(
            tab,
            evidence(vec![row(7, 1, true, Some(10)), row(8, 7, false, Some(5))]),
            61_000,
        );
        assert_eq!(
            (stat.memory_bytes, stat.elapsed_seconds),
            (Some(15), Some(60.0))
        );
        assert_eq!(
            (
                stat.processes[1].parent_pid,
                stat.processes[1].name.as_str()
            ),
            (7, "p8")
        );
        // One unmeasured process leaves the total unknown, not too small.
        let partial = stats(
            tab,
            evidence(vec![row(7, 1, true, Some(10)), row(8, 7, false, None)]),
            61_000,
        );
        assert_eq!(partial.memory_bytes, None);
    }
}
