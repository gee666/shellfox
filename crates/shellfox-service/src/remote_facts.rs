//! Bounded async fact collection from immutable owner evidence, never &Service.
#[cfg(test)]
#[path = "remote_facts_tests.rs"]
mod tests;
use super::*;
use shellfox_runtime::{Identity, Phase};
use std::{future::Future, pin::Pin, sync::Arc};
#[derive(Clone)]
pub struct RuntimeFactIdentity {
    pub identity: Identity,
    pub phase: Phase,
}
#[derive(Clone)]
pub struct ManagerFactsRequest {
    /// Internal response scope only. Snapshot itself always remains complete.
    pub target_session: Option<SessionId>,
    /// Actual owner local-only tab exclusions, never a dropped session inventory.
    pub private_tabs: std::collections::HashSet<shellfox_core::TabId>,
    pub snapshot: Snapshot,
    pub settings: shellfox_core::Settings,
    pub terminals: Vec<RuntimeFactIdentity>,
    pub cancelled: wire::CancellationToken,
}
pub struct ManagerFactsResult {
    pub facts: ManagerFacts,
    pub profiles: Option<wire::TerminalProfiles>,
    pub home: Option<wire::Directory>,
}
pub type ManagerFactsFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ManagerFactsResult, AppError>> + Send + 'a>>;
/// Implement with actual discovery/owned process providers. Missing evidence
/// remains absent. No default Running state, copied tab timestamp or numeric-PID authority.
pub trait ManagerFactsProvider: Send + Sync {
    fn collect(&self, request: ManagerFactsRequest) -> ManagerFactsFuture<'_>;
    /// Actual native/system capabilities without the strict V1 session mapper.
    fn collect_capabilities(&self, _request: ManagerFactsRequest) -> ManagerFactsFuture<'_> {
        Box::pin(async {
            Err(capability_unavailable(
                "independent native capability collection",
            ))
        })
    }
}
pub(super) struct FactsJob {
    pub request: shellfox_remote::OwnerRequest,
    pub evidence: ManagerFactsRequest,
    pub result: Result<ManagerFactsResult, AppError>,
    pub capabilities_only: bool,
}
pub(super) struct FactsPipeline {
    pub provider: Arc<dyn ManagerFactsProvider>,
    pub runtime: tokio::runtime::Handle,
    pub sender: tokio::sync::mpsc::Sender<FactsJob>,
    pub receiver: tokio::sync::mpsc::Receiver<FactsJob>,
    pub pending: usize,
    stop: wire::CancellationToken,
}
impl Drop for FactsPipeline {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl FactsPipeline {
    pub fn new(provider: Arc<dyn ManagerFactsProvider>, runtime: tokio::runtime::Handle) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        Self {
            provider,
            runtime,
            sender,
            receiver,
            pending: 0,
            stop: wire::CancellationToken::new(),
        }
    }
    /// `wake` is notified when the collected job is ready for [`Self::finish`].
    pub fn admit(
        &mut self,
        service: &mut Service,
        request: shellfox_remote::OwnerRequest,
        wake: crate::OwnerWakeSlot,
    ) {
        if request.is_cancelled() {
            return;
        }
        if self.pending >= 8 {
            request.complete(Err(failure(
                ErrorCode::NativeUnavailable,
                "Fact collection queue full. No mutation was executed.",
            )));
            return;
        }
        let evidence = (|| {
            let snapshot = service.host().snapshot().map_err(host_error)?;
            let terminals = identities(service, &snapshot);
            let target_session = required_session(&request.command, &snapshot)?;
            Ok(ManagerFactsRequest {
                target_session,
                private_tabs: snapshot
                    .sessions
                    .iter()
                    .flat_map(|session| session.tabs.iter())
                    .filter(|tab| {
                        super::privacy::elevated_profile(tab.profile_id.as_str()) || service.terminals.get(&tab.id).is_some_and(|terminal| {
                            terminal.runtime.identity().session_id == tab.session_id
                                && super::v1_privilege::explicitly_elevated(
                                    &terminal.runtime.launch_privilege_facts())
                        })
                    })
                    .map(|tab| tab.id)
                    .collect(),
                snapshot,
                settings: service.host().global_settings().map_err(host_error)?,
                terminals,
                cancelled: service.ownership_lost.child_token(),
            })
        })();
        let evidence = match evidence {
            Ok(evidence) => evidence,
            Err(error) => {
                request.complete(Err(error));
                return;
            }
        };
        let actor = request.command.actor().clone();
        if let Err(error) = actor.with_authorized(request.command.operation(), || Ok(())) {
            request.complete(Err(error));
            return;
        }
        // Catalog writes need to be visible to this request, independent of the
        // next activity poll. Genuine changes during projection are still fenced.
        let source = crate::activity_integration::read_source(&service.source_facts_watch());
        if service.activity.dirty || source.is_none_or(|source| !source.matches_request(&evidence)) {
            let _ = service.publish_source_facts(evidence.snapshot.clone(), evidence.settings.clone());
        }
        self.pending += 1;
        let capabilities_only = capabilities_only(&request.command);
        let provider = self.provider.clone();
        let sender = self.sender.clone();
        let input = evidence.clone();
        let stop = self.stop.clone();
        let collected=self.runtime.spawn(async move {
            tokio::select! {
                _=stop.cancelled()=>Err(failure(ErrorCode::NativeUnavailable,"Remote dispatcher stopped during fact collection.")),
                _=input.cancelled.cancelled()=>Err(failure(ErrorCode::NativeUnavailable,"Owner lost during fact collection.")),
                result=tokio::time::timeout(std::time::Duration::from_secs(15),async { if capabilities_only { provider.collect_capabilities(input.clone()).await } else { provider.collect(input.clone()).await } })=>result.unwrap_or_else(|_|Err(capability_unavailable("owner facts collection deadline"))),
            }
        });
        self.runtime.spawn(async move {
            let result = collected.await.unwrap_or_else(|_| {
                Err(failure(
                    ErrorCode::NativeUnavailable,
                    "Owner fact provider stopped. No mutation was executed.",
                ))
            });
            let _ = sender
                .send(FactsJob {
                    request,
                    evidence,
                    result,
                    capabilities_only,
                })
                .await;
            wake.notify();
        });
    }
    pub fn finish(&mut self, service: &mut Service, state: &mut RemoteState) -> bool {
        let Ok(job) = self.receiver.try_recv() else {
            return false;
        };
        self.pending = self.pending.saturating_sub(1);
        if job.request.is_cancelled() {
            return true;
        }
        let result = (|| {
            let snapshot = service.host().snapshot().map_err(host_error)?;
            if snapshot.revision != job.evidence.snapshot.revision
                || !same_runtime(&identities(service, &snapshot), &job.evidence.terminals)
            {
                return Err(failure(ErrorCode::TargetLost,"Owner state changed while facts were collected. Refresh explicitly; mutation was not executed."));
            }
            job.result
        })();
        match result {
            Ok(facts) => {
                if job.capabilities_only {
                    state.facts.settings = facts.facts.settings;
                    state.facts.probe = facts.facts.probe;
                    state.facts.explorer = facts.facts.explorer;
                    state.facts.cli = facts.facts.cli;
                } else {
                    state.facts = facts.facts;
                }
                state.profiles = facts.profiles;
                state.home = facts.home;
                state.dispatch_request(service, job.request);
            }
            Err(error) => job.request.complete(Err(error)),
        }
        true
    }
}
fn required_session(
    command: &OwnerCommand,
    snapshot: &Snapshot,
) -> Result<Option<SessionId>, AppError> {
    use wire::RemoteCommand::*;
    let OwnerCommand::Execute { command, .. } = command else {
        return Ok(None);
    };
    Ok(match command {
        RenameSession(v) => Some(v.session_id),
        RenameTab(v) => Some(v.session_id),
        ReorderTabs(v) => Some(v.session_id),
        SetSessionPinned(v) => Some(v.session_id),
        SettleSession(v) => Some(v.session_id),
        UnsettleSession(v) => Some(v.session_id),
        SetSessionEnv(v) => Some(v.session_id),
        DeleteSession(v) => Some(v.session_id),
        GetSessionStats(v) => v.session_id,
        CloseTab(v) => Some(
            snapshot
                .sessions
                .iter()
                .find(|session| session.tabs.iter().any(|tab| tab.id == v.tab_id))
                .map(|session| session.id)
                .ok_or_else(|| {
                    failure(
                        ErrorCode::NotFound,
                        "Target tab is not in the owner snapshot.",
                    )
                })?,
        ),
        _ => None,
    })
}
fn capabilities_only(command: &OwnerCommand) -> bool {
    let OwnerCommand::Execute { command, .. } = command else {
        return false;
    };
    use wire::RemoteCommand::*;
    match command {
        GetTerminalProfiles(_) | GetHomeDirectory(_) | DeleteSession(_)
        | AttachTerminal(_) | CreateSession(_) | AddTab(_) | ActivateSession(_) => true,
        ListFiles(v) => v.session_id.is_some() || v.tab_id.is_some(),
        ReadFile(v) => v.session_id.is_some() || v.tab_id.is_some(),
        WriteFile(v) => v.session_id.is_some() || v.tab_id.is_some(),
        ReadFileAsset(v) => v.session_id.is_some() || v.tab_id.is_some(),
        WriteImageFile(v) => v.session_id.is_some() || v.tab_id.is_some(),
        BeginFileRead(v) => v.session_id.is_some() || v.tab_id.is_some(),
        BeginFileWrite(v) => v.session_id.is_some() || v.tab_id.is_some(),
        CreateDirectory(v) => v.session_id.is_some() || v.tab_id.is_some(),
        _ => false,
    }
}
fn identities(service: &Service, snapshot: &shellfox_core::Snapshot) -> Vec<RuntimeFactIdentity> {
    let mut identities: Vec<_> = service
        .terminals
        .values()
        .filter(|terminal| crate::facts_membership::contains(snapshot, terminal.runtime.identity()))
        .map(|terminal| RuntimeFactIdentity {
            identity: terminal.runtime.identity(),
            phase: terminal.runtime.phase(),
        })
        .collect();
    identities.sort_by_key(|item| item.identity.tab_id);
    identities
}
fn same_runtime(left: &[RuntimeFactIdentity], right: &[RuntimeFactIdentity]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| a.identity == b.identity && a.phase == b.phase)
}
pub(super) fn requires_facts(
    command: &OwnerCommand,
    has_runtime: impl FnOnce(wire::TabId) -> bool,
) -> bool {
    if let OwnerCommand::Execute { command: wire::RemoteCommand::AttachTerminal(input), .. } = command {
        // Only a cold attach needs to resolve a launch profile. A live attach
        // validates and adopts the runtime on this same owner turn.
        return !has_runtime(input.tab_id);
    }
    if capabilities_only(command) {
        return true;
    }
    matches!(
        command,
        OwnerCommand::Execute {
            command: wire::RemoteCommand::GetSnapshot(_)
                | wire::RemoteCommand::GetSessionStats(_)
                | wire::RemoteCommand::RenameSession(_)
                | wire::RemoteCommand::RenameTab(_)
                | wire::RemoteCommand::ReorderTabs(_)
                | wire::RemoteCommand::SetSessionPinned(_)
                | wire::RemoteCommand::SettleSession(_)
                | wire::RemoteCommand::UnsettleSession(_)
                | wire::RemoteCommand::SetSessionEnv(_)
                | wire::RemoteCommand::CloseTab(_)
                | wire::RemoteCommand::DeleteSession(_),
            ..
        }
    )
}
