//! Create, addTab and activateSession replies that wait for their new shell,
//! as 0.9 replied only once it was open. The owner keeps serving other
//! requests meanwhile.
use crate::{local_api::LocalApiResponse, Service};
use anyhow::Result;
use shellfox_core::{SessionId, Snapshot, TabId, TabState};
use shellfox_host::HostRequest;
use shellfox_runtime::Phase;

/// Sends one local API reply, on whichever route the request came.
pub(crate) type LaunchReply = Box<dyn FnOnce(Result<LocalApiResponse>) + Send>;

/// A tab created through the local API whose shell failed to start: 0.9's
/// LAUNCH_FAILED. The saved tab stays, as in 0.9.
#[derive(Debug)]
pub struct LaunchFailure(pub String);
impl std::fmt::Display for LaunchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LaunchFailure {}

pub(crate) fn failure(error: &anyhow::Error) -> anyhow::Error {
    let cause = error.root_cause().to_string();
    anyhow::Error::new(LaunchFailure(format!(
        "The shell could not be started: {cause}"
    )))
}

impl Service {
    /// Replies at once, or once the start the request began has settled.
    pub(crate) fn reply_after_launch(
        &mut self,
        result: Result<LocalApiResponse>,
        reply: LaunchReply,
    ) {
        match self.launch_begun.take() {
            Some(tab) if result.is_ok() => self.launch_replies.push((tab, reply)),
            _ => reply(result),
        }
    }
    /// Answers each waiting reply whose start settled: the fresh snapshot, or
    /// the start's failure.
    pub(crate) fn finish_launches(&mut self) {
        let mut index = 0;
        while index < self.launch_replies.len() {
            if !self.start_settled(self.launch_replies[index].0) {
                index += 1;
                continue;
            }
            let (tab, reply) = self.launch_replies.swap_remove(index);
            let result = match self.start_terminal(tab) {
                Ok(()) => self
                    .host()
                    .snapshot()
                    .map(LocalApiResponse::Snapshot)
                    .map_err(Into::into),
                Err(error) => Err(failure(&error)),
            };
            reply(result);
        }
    }
}

impl Service {
    /// A tab created here starts its shell at once, as in 0.9 and on the peer
    /// route, so CLI and automation can write to it. Restored tabs still start
    /// on their first attach, like 0.9's sessions after a restart.
    pub(crate) fn start_new_tab(&mut self, before: &Snapshot, after: Snapshot) -> Result<Snapshot> {
        let existed = |id: TabId| {
            before
                .sessions
                .iter()
                .any(|s| s.tabs.iter().any(|t| t.id == id))
        };
        let created = after
            .sessions
            .iter()
            .flat_map(|s| &s.tabs)
            .map(|t| t.id)
            .find(|id| !existed(*id));
        match created {
            Some(tab) => self.start_replying(tab),
            None => Ok(after),
        }
    }
    /// 0.9's activateSession: selects the session and, when none of its shells
    /// runs, opens its first tab that a plain start may launch, or a new tab
    /// when it has none, as after its last shell exited. Automation and
    /// Administrator tabs are never started this way.
    pub(crate) fn activate_session(&mut self, id: SessionId) -> Result<Snapshot> {
        let snapshot =
            self.application_metadata(HostRequest::ActivateSession { session_id: id })?;
        let Some(session) = snapshot.sessions.iter().find(|s| s.id == id) else {
            return Ok(snapshot);
        };
        let running = session.tabs.iter().any(|t| {
            self.terminals
                .get(&t.id)
                .is_some_and(|terminal| terminal.runtime.phase() != Phase::Closed)
        });
        let first = session.tabs.iter().find(|t| {
            t.state != TabState::Archived
                && !crate::automation::replay::is_automation_tab(t)
                && t.profile_id.as_str() != crate::ADMIN_POWERSHELL_PROFILE
        });
        let automation = session
            .tabs
            .iter()
            .any(crate::automation::replay::is_automation_tab);
        match first {
            _ if running => Ok(snapshot),
            Some(tab) => self.start_replying(tab.id),
            None if automation => Ok(snapshot),
            None => {
                let profile_id = self
                    .folder_launch_profile(&snapshot.preferences.default_profile_id, &session.cwd)
                    .map_err(|e| failure(&e))?;
                let input = shellfox_core::AddTab {
                    title: None,
                    cwd: None,
                    profile_id,
                };
                let after = self.application_metadata(HostRequest::AddTab {
                    session_id: id,
                    input,
                })?;
                self.start_new_tab(&snapshot, after)
            }
        }
    }
    /// Starts `tab` without blocking the owner: the owner holds this reply
    /// until the shell is open. A failed start is LAUNCH_FAILED; the saved tab
    /// stays, as in 0.9.
    fn start_replying(&mut self, tab: TabId) -> Result<Snapshot> {
        if self.begin_start(tab).map_err(|e| failure(&e))? {
            self.launch_begun = Some(tab);
        } else {
            // Already settled: a start that failed this fast (macOS rejects a
            // missing cwd at once) is reported here, never as a started tab.
            self.start_terminal(tab).map_err(|e| failure(&e))?;
        }
        Ok(self.host().snapshot()?)
    }
}
