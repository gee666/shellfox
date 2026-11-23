//! Closing tabs and sessions without blocking the owner. A close prepares
//! its metadata token, asks every owned runtime to stop, and then finishes on
//! a later owner pass once each one reports Closed. Until then the owner keeps
//! serving reads and terminal I/O for other tabs; everything else waits behind
//! the close (see `Service::metadata_busy`), because any metadata commit in
//! between would invalidate the prepared token.
use crate::{local_api::LocalApiRequest, native::NativeAction, Service};
use anyhow::{bail, Context, Result};
use shellfox_core::{CleanupToken, SessionId, Snapshot, TabId, TerminalIdentity};
use shellfox_runtime::{Phase, SourceKind};
use std::time::{Duration, Instant};

/// How long owned runtimes get to confirm their stop.
const STOP_DEADLINE: Duration = Duration::from_secs(12);

#[derive(Clone, Copy, Debug)]
pub(crate) enum CloseTarget {
    Tab(TabId),
    /// Only if this exact generation is still current.
    Terminal(TerminalIdentity),
    Session(SessionId),
}
/// Receives the close's outcome exactly once, on the owner thread.
pub(crate) type CloseDone = Box<dyn FnOnce(Result<Snapshot>) + Send>;
/// Runs the metadata commit it is given, or refuses it (a peer's close
/// commits only while the peer is still authorized).
pub(crate) type CommitGate =
    Box<dyn FnOnce(&dyn Fn() -> Result<Snapshot>) -> Result<Snapshot> + Send>;

struct Started {
    token: CleanupToken,
    tabs: Vec<TabId>,
    session: Option<SessionId>,
    terminals: Vec<TerminalIdentity>,
}
pub(crate) struct Closing {
    token: CleanupToken,
    tabs: Vec<TabId>,
    session: Option<SessionId>,
    /// Runtimes asked to stop that haven't confirmed yet.
    waiting: Vec<TerminalIdentity>,
    deadline: Instant,
    gate: Option<CommitGate>,
    done: CloseDone,
}

impl Service {
    /// True while a close is in flight. Requests that could write metadata or
    /// touch the closing tabs wait until it finishes.
    pub(crate) fn metadata_busy(&self) -> bool {
        self.closing.is_some()
    }
    /// Whether `tab` belongs to the close in flight.
    pub(crate) fn tab_closing(&self, tab: TabId) -> bool {
        self.closing.as_ref().is_some_and(|closing| closing.tabs.contains(&tab))
    }
    /// When the close in flight gives up, for the owner's pass timeout.
    pub(crate) fn close_deadline(&self) -> Option<Instant> {
        self.closing.as_ref().map(|closing| closing.deadline)
    }
    /// Starts a close. `done` runs once it commits or fails, possibly before
    /// this returns.
    pub(crate) fn begin_close(&mut self, target: CloseTarget, done: CloseDone) {
        self.begin_close_gated(target, None, done);
    }
    fn begin_close_gated(&mut self, target: CloseTarget, gate: Option<CommitGate>, done: CloseDone) {
        match self.start_close(target) {
            Ok(Started { token, tabs, session, terminals }) => {
                // A gated close starts inside its caller's authorization check,
                // which the gate takes again: the caller pumps it afterwards.
                let pump = gate.is_none();
                self.metadata_closing = true;
                self.closing = Some(Closing {
                    token,
                    tabs,
                    session,
                    waiting: terminals,
                    deadline: Instant::now() + STOP_DEADLINE,
                    gate,
                    done,
                });
                if pump {
                    self.pump_closing();
                }
            }
            Err(error) => done(Err(error)),
        }
    }
    fn start_close(&mut self, target: CloseTarget) -> Result<Started> {
        self.assert_owned()?;
        if self.metadata_closing {
            bail!("Metadata cleanup is already active.");
        }
        let (tabs, session, prepared) = match target {
            CloseTarget::Tab(id) => (vec![id], None, self.host.prepare_close_tab(id)?),
            CloseTarget::Terminal(identity) => (
                vec![identity.tab_id],
                None,
                self.host.prepare_close_terminal(identity)?,
            ),
            CloseTarget::Session(id) => {
                let tabs = self
                    .host
                    .snapshot()?
                    .sessions
                    .iter()
                    .find(|session| session.id == id)
                    .map(|session| session.tabs.iter().map(|tab| tab.id).collect())
                    .unwrap_or_default();
                (tabs, Some(id), self.host.prepare_delete_session(id)?)
            }
        };
        let started = tabs
            .iter()
            .try_for_each(|tab| self.close_external_for_tab(*tab))
            .and_then(|()| {
                prepared
                    .terminals
                    .iter()
                    .try_for_each(|identity| self.begin_terminal_stop(*identity))
            });
        if let Err(error) = started {
            self.host.cancel_close(prepared.token)?;
            return Err(error);
        }
        Ok(Started { token: prepared.token, tabs, session, terminals: prepared.terminals })
    }
    /// Advances the close in flight. Runs on every owner pass.
    pub(crate) fn pump_closing(&mut self) {
        let Some(mut closing) = self.closing.take() else {
            return;
        };
        let result = match self.advance_close(&mut closing) {
            Ok(None) => {
                self.closing = Some(closing);
                return;
            }
            Ok(Some(snapshot)) => Ok(snapshot),
            Err(error) => self.host.cancel_close(closing.token).map_err(Into::into).and(Err(error)),
        };
        self.metadata_closing = false;
        (closing.done)(result);
    }
    /// Fails the close in flight, if any, without waiting for its runtimes.
    pub(crate) fn abandon_close(&mut self, reason: &str) {
        if let Some(closing) = self.closing.take() {
            let _ = self.host.cancel_close(closing.token);
            self.metadata_closing = false;
            (closing.done)(Err(anyhow::anyhow!("{reason}")));
        }
    }
    fn advance_close(&mut self, closing: &mut Closing) -> Result<Option<Snapshot>> {
        let mut index = 0;
        while index < closing.waiting.len() {
            if self.confirm_stopped(closing.waiting[index])? {
                closing.waiting.swap_remove(index);
            } else {
                index += 1;
            }
        }
        if !closing.waiting.is_empty() {
            if Instant::now() >= closing.deadline {
                bail!("Owned terminal cleanup not confirmed; metadata retained.");
            }
            return Ok(None);
        }
        // Native loss while waiting must never permit the metadata commit.
        self.assert_owned()?;
        let (host, token) = (&self.host, closing.token);
        let commit = || host.commit_close(token).map_err(anyhow::Error::from);
        let snapshot = match closing.gate.take() {
            Some(gate) => gate(&commit)?,
            None => commit()?,
        };
        for tab in &closing.tabs {
            self.forget_closed_external(*tab);
        }
        if let Some(session) = closing.session {
            self.activity.persistence.forget_session(session);
        }
        Ok(Some(snapshot))
    }
    /// Asks one owned runtime to stop. External-byte sources close in
    /// `confirm_stopped` instead.
    pub(crate) fn begin_terminal_stop(&mut self, identity: TerminalIdentity) -> Result<()> {
        let terminal = self
            .terminals
            .get_mut(&identity.tab_id)
            .context("Registered terminal owner is missing; cleanup cannot be confirmed.")?;
        let runtime = &terminal.runtime;
        if runtime.identity().generation != identity.generation {
            bail!("Terminal generation changed.");
        }
        if runtime.source_kind() != SourceKind::ExternalBytes && runtime.phase() != Phase::Closed {
            terminal.discard_output();
            self.request_close(identity.tab_id)?;
        }
        Ok(())
    }
    /// True once the runtime reports Closed; then forgets it. Never waits.
    pub(crate) fn confirm_stopped(&mut self, identity: TerminalIdentity) -> Result<bool> {
        let terminal = self
            .terminals
            .get_mut(&identity.tab_id)
            .context("Terminal owner disappeared.")?;
        if terminal.runtime.source_kind() == SourceKind::ExternalBytes {
            self.close_external_parser(identity)?;
            return Ok(true);
        }
        terminal.discard_output();
        self.drain(identity.tab_id)?;
        let terminal = self
            .terminals
            .get(&identity.tab_id)
            .context("Terminal owner disappeared.")?;
        if terminal.runtime.phase() != Phase::Closed {
            return Ok(false);
        }
        let runtime_identity = terminal.runtime.identity();
        self.host.terminal_stopped(identity)?;
        self.activity_collect_live();
        self.activity_confirmed_exit(runtime_identity);
        self.forget_activity(runtime_identity);
        self.terminals.remove(&identity.tab_id);
        self.ownership_source_destroyed(runtime_identity);
        Ok(true)
    }
    /// Blocking stop of one runtime, for automation retirement.
    pub(crate) fn stop_terminal(&mut self, identity: TerminalIdentity) -> Result<()> {
        self.begin_terminal_stop(identity)?;
        self.confirm_terminal_stop(identity, Instant::now() + STOP_DEADLINE)
    }
    /// Blocking form for shutdown.
    pub(crate) fn confirm_terminal_stop(
        &mut self,
        identity: TerminalIdentity,
        deadline: Instant,
    ) -> Result<()> {
        self.wait_until(
            deadline,
            |service| Ok(service.confirm_stopped(identity)?.then_some(())),
            || anyhow::anyhow!("Owned terminal cleanup not confirmed; metadata retained."),
        )
    }
    /// Blocking form of `begin_close`, for callers that have no reply to defer.
    pub(crate) fn close_now(&mut self, target: CloseTarget) -> Result<Snapshot> {
        let (send, receive) = std::sync::mpsc::channel();
        self.begin_close(target, Box::new(move |result| {
            let _ = send.send(result);
        }));
        self.wait_until(
            Instant::now() + STOP_DEADLINE + Duration::from_secs(1),
            |service| {
                if let Ok(result) = receive.try_recv() {
                    return result.map(Some);
                }
                service.pump_closing();
                Ok(None)
            },
            || anyhow::anyhow!("Owned terminal cleanup not confirmed; metadata retained."),
        )
    }
}

impl LocalApiRequest {
    pub(crate) fn close_target(&self) -> Option<CloseTarget> {
        match self {
            Self::CloseTab { id } => Some(CloseTarget::Tab(*id)),
            Self::DeleteSession { id } => Some(CloseTarget::Session(*id)),
            _ => None,
        }
    }
}
impl NativeAction {
    pub(crate) fn close_target(&self) -> Option<CloseTarget> {
        match self {
            Self::CloseTab { tab } => Some(CloseTarget::Tab(*tab)),
            Self::CloseTerminal(identity) => Some(CloseTarget::Terminal(*identity)),
            Self::DeleteSession { session } => Some(CloseTarget::Session(*session)),
            _ => None,
        }
    }
}
impl Service {
    /// Starts the close a request names, with the checks its synchronous
    /// route made first.
    pub(crate) fn close_request(&mut self, target: CloseTarget, done: CloseDone) {
        if let Err(error) = self.assert_accepting() {
            return done(Err(error));
        }
        self.begin_close(target, done);
    }
    /// `close_request` whose commit runs through `gate`.
    pub(crate) fn close_request_gated(&mut self, target: CloseTarget, gate: CommitGate, done: CloseDone) {
        if let Err(error) = self.assert_accepting() {
            return done(Err(error));
        }
        self.begin_close_gated(target, Some(gate), done);
    }
    /// Reads and terminal I/O on other tabs may run during a close.
    pub(crate) fn local_api_flows(&self, request: &LocalApiRequest) -> bool {
        use LocalApiRequest::*;
        match request {
            Snapshot {} => true,
            Poll { tab, .. }
            | Acknowledge { tab, .. }
            | Recover { tab, .. }
            | Input { tab, .. }
            | Key { tab, .. }
            | Paste { tab, .. }
            | Resize { tab, .. } => !self.tab_closing(*tab),
            _ => false,
        }
    }
    /// Status reads, frames and terminal I/O on other tabs may run during a close.
    pub(crate) fn native_flows(&self, action: &NativeAction) -> bool {
        use NativeAction::*;
        match action {
            Status | ReadWindowState | ReadTabActivity | ChangedSince { .. } | TabStats => true,
            Frame { tab, .. }
            | AcceptFrame { tab, .. }
            | ObserverFrame { tab, .. }
            | AcceptObserverFrame { tab, .. }
            | Presentation { tab, .. }
            | Terminal { tab, .. } => !self.tab_closing(*tab),
            _ => false,
        }
    }
}

#[cfg(test)]
impl Service {
    /// A close in flight that never advances: nothing to wait for, no pump.
    pub(crate) fn park_close(&mut self, tab: TabId) -> std::sync::mpsc::Receiver<Result<Snapshot>> {
        let (send, outcome) = std::sync::mpsc::channel();
        let prepared = self.host.prepare_close_tab(tab).unwrap();
        self.closing = Some(Closing {
            token: prepared.token,
            tabs: vec![tab],
            session: None,
            waiting: prepared.terminals,
            deadline: Instant::now() + STOP_DEADLINE,
            gate: None,
            done: Box::new(move |result| {
                let _ = send.send(result);
            }),
        });
        self.metadata_closing = true;
        outcome
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{default_catalog_profile, scratch};
    use shellfox_core::{AddTab, CreateSession};

    #[test]
    fn a_close_in_flight_lets_reads_through_and_holds_metadata_writes() {
        let data = scratch();
        let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
        // A failed launch still registers a runtime that the close must stop.
        let cwd = data.path().join("missing-cwd").to_string_lossy().into_owned();
        let profile_id = shellfox_core::ProfileId::new("cmd").unwrap();
        let created = service
            .host()
            .create_session(CreateSession { request_id: SessionId::new(), title: None, cwd, profile_id: profile_id.clone() })
            .unwrap();
        let session = created.sessions[0].id;
        let (first, second) = {
            let snapshot = service.host().add_tab(session, AddTab { title: None, cwd: None, profile_id }).unwrap();
            (snapshot.sessions[0].tabs[0].id, snapshot.sessions[0].tabs[1].id)
        };
        assert!(service.attach(first, 1, false).is_err());
        assert!(service.attach(second, 1, false).is_err());
        assert_eq!(service.active_terminals(), 2);

        // While a close waits for its runtime: reads and other tabs' I/O run.
        let _parked = service.park_close(first);
        {
            assert!(service.tab_closing(first) && !service.tab_closing(second));
            let attachment = crate::Attachment { controller: 1, epoch: 1, generation: Default::default() };
            let read = |tab| LocalApiRequest::Poll { tab, attachment: attachment.clone() };
            assert!(service.local_api_flows(&LocalApiRequest::Snapshot {}));
            assert!(service.local_api_flows(&read(second)), "I/O on another tab runs");
            assert!(!service.local_api_flows(&read(first)), "the closing tab waits");
            let rename = LocalApiRequest::RenameSession { id: session, title: "later".into() };
            assert!(!service.local_api_flows(&rename), "metadata writes wait");
            assert!(service.host().snapshot().is_ok());
            // A second close can't start while the first holds its token.
            assert!(service.close_tab(second).is_err());
        }
        service.abandon_close("test");
        let (send, outcome) = std::sync::mpsc::channel();
        service.begin_close(CloseTarget::Tab(first), Box::new(move |result| {
            let _ = send.send(result);
        }));
        let deadline = Instant::now() + STOP_DEADLINE;
        let closed = loop {
            if let Ok(result) = outcome.try_recv() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "close did not finish");
            service.pump();
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(closed.sessions[0].tabs.len(), 1);
        assert!(!service.metadata_busy() && service.terminals.contains_key(&second));
        service.close_tab(second).unwrap();
        service.shutdown().unwrap();
    }

    #[test]
    fn shutdown_fails_a_close_in_flight_and_keeps_its_metadata() {
        let data = scratch();
        let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
        let cwd = data.path().to_string_lossy().into_owned();
        let created = service
            .host()
            .create_session(CreateSession { request_id: SessionId::new(), title: None, cwd, profile_id: default_catalog_profile() })
            .unwrap();
        let tab = created.sessions[0].tabs[0].id;
        // No live terminal: the close would finish on its first pass, so park it.
        let outcome = service.park_close(tab);
        service.shutdown().unwrap();
        assert!(outcome.try_recv().unwrap().is_err());
        assert_eq!(service.host().snapshot().unwrap().sessions[0].tabs.len(), 1);
    }
}
