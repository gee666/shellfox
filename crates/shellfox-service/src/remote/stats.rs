use super::*;
use shellfox_remote::OwnerRequest;
use tokio::sync::oneshot;

type Measured = anyhow::Result<Vec<wire::TabStat>>;

/// Peer `getSessionStats` requests waiting for the tab-stats worker. Each one
/// is dispatched again, authorized as usual, once its measurements arrive.
#[derive(Default)]
pub(super) struct PendingStats {
    waiting: Vec<(OwnerRequest, oneshot::Receiver<Measured>)>,
    /// Set only while a finished request is being dispatched.
    pub(super) measured: Option<Measured>,
}
pub(super) fn is_stats(command: &OwnerCommand) -> bool {
    matches!(command, OwnerCommand::Execute { command: RemoteCommand::GetSessionStats(_), .. })
}
impl RemoteState {
    /// None when the request waits for a capture first.
    pub(super) fn stats_ready(&mut self, service: &mut Service, request: OwnerRequest) -> Option<OwnerRequest> {
        if self.stats.measured.is_some() || !is_stats(&request.command) {
            return Some(request);
        }
        if !request.is_cancelled() {
            let (reply, receiver) = oneshot::channel();
            service.enqueue_tab_stats(crate::tab_stats::StatsReply::Remote(reply));
            self.stats.waiting.push((request, receiver));
        }
        None
    }
    /// Dispatches every request whose capture finished.
    pub(super) fn finish_stats(&mut self, service: &mut Service) {
        let mut index = 0;
        while index < self.stats.waiting.len() {
            let measured = match self.stats.waiting[index].1.try_recv() {
                Ok(measured) => measured,
                Err(oneshot::error::TryRecvError::Empty) => {
                    index += 1;
                    continue;
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    Err(anyhow::anyhow!("The stats capture could not start."))
                }
            };
            let (request, _) = self.stats.waiting.swap_remove(index);
            self.stats.measured = Some(measured);
            self.dispatch_request(service, request);
            self.stats.measured = None;
        }
    }
}
impl RemoteService<'_> {
    pub(super) fn statistics(
        &mut self,
        input: wire::GetSessionStats,
    ) -> Result<serde_json::Value, AppError> {
        let snapshot = self.service.host().snapshot().map_err(host_error)?;
        let ids = if let Some(id) = input.session_id {
            vec![id]
        } else {
            snapshot
                .sessions
                .iter()
                .map(|session| session.id)
                .filter(|id| !self.state.facts.omitted_sessions.contains(id))
                .collect()
        };
        let measured: Result<HashMap<_, _>, String> = match self.state.stats.measured.take() {
            Some(Ok(stats)) => Ok(stats.into_iter().map(|stat| (stat.tab_id, stat)).collect()),
            Some(Err(error)) => Err(error.to_string()),
            None => Err("Process measurements are unavailable.".into()),
        };
        let mut sessions = Vec::new();
        let mut tabs = Vec::new();
        for id in ids {
            let mut session = self.session(&snapshot, id)?;
            session.env.clear();
            for tab in &session.tabs {
                let reason = match &measured {
                    _ if self.elevated(&snapshot, tab.id) => {
                        "Process measurements are not shared for elevated tabs."
                    }
                    Ok(stats) => match stats.get(&tab.id) {
                        Some(stat) => {
                            tabs.push(stat.clone());
                            continue;
                        }
                        None => "The terminal is not running.",
                    },
                    Err(reason) => reason,
                };
                tabs.push(wire::TabStat {
                    tab_id: tab.id,
                    processes: Vec::new(),
                    memory_bytes: None,
                    elapsed_seconds: None,
                    reason: Some(reason.into()),
                });
            }
            sessions.push(session);
        }
        if tabs.len() > 64 {
            return Err(capability_unavailable("complete statistics inventory exceeds the bounded response; no partial rows were returned"));
        }
        checked(wire::SessionStats { sessions, tabs })
    }
    /// The same rule the facts pipeline uses to keep elevated tabs private.
    fn elevated(&self, snapshot: &Snapshot, tab: shellfox_core::TabId) -> bool {
        snapshot
            .sessions
            .iter()
            .flat_map(|session| &session.tabs)
            .find(|candidate| candidate.id == tab)
            .is_some_and(|candidate| {
                super::privacy::elevated_profile(candidate.profile_id.as_str())
                    || self.service.terminals.get(&tab).is_some_and(|terminal| {
                        terminal.runtime.identity().session_id == candidate.session_id
                            && super::v1_privilege::explicitly_elevated(
                                &terminal.runtime.launch_privilege_facts(),
                            )
                    })
            })
    }
}
