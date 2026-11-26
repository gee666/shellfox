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
    /// The request that started the close: its commit is that request's change.
    cause: Option<crate::owner::Cause>,
    /// Test-only: a parked close never advances.
    #[cfg(test)]
    parked: bool,
}

impl Service {
    /// True from a close's prepare until its commit or cancel. Requests that
    /// could write metadata or touch the closing tabs wait until it finishes.
    pub(crate) fn metadata_busy(&self) -> bool {
        self.metadata_closing
    }
    /// Whether `tab` belongs to the close in flight.
    pub(crate) fn tab_closing(&self, tab: TabId) -> bool {
        self.closing
            .as_ref()
            .is_some_and(|closing| closing.tabs.contains(&tab))
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
    fn begin_close_gated(
        &mut self,
        target: CloseTarget,
        gate: Option<CommitGate>,
        done: CloseDone,
    ) {
        if let Err(error) = self.assert_owned() {
            return done(Err(error));
        }
        if self.metadata_closing {
            return done(Err(anyhow::anyhow!("Metadata cleanup is already active.")));
        }
        // Set before prepare: stopping an automation tab pumps the owner, and
        // nothing may write metadata between prepare and commit.
        self.metadata_closing = true;
        match self.start_close(target) {
            Ok(Started {
                token,
                tabs,
                session,
                terminals,
            }) => {
                // A gated close starts inside its caller's authorization check,
                // which the gate takes again (that lock isn't reentrant), so the
                // caller pumps it afterwards. No pump may run under that check.
                let pump = gate.is_none();
                self.closing = Some(Closing {
                    token,
                    tabs,
                    session,
                    waiting: terminals,
                    deadline: Instant::now() + STOP_DEADLINE,
                    gate,
                    done,
                    cause: self.durability.current(),
                    #[cfg(test)]
                    parked: false,
                });
                if pump {
                    self.pump_closing();
                }
            }
            Err(error) => {
                self.metadata_closing = false;
                done(Err(error))
            }
        }
    }
    /// Prepares the token and asks every runtime to stop. If one can't be
    /// asked, the token is cancelled; runtimes already asked stay registered
    /// until a retried close confirms them.
    fn start_close(&mut self, target: CloseTarget) -> Result<Started> {
        #[cfg(test)]
        tests::panic_if_marked(target);
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
        Ok(Started {
            token: prepared.token,
            tabs,
            session,
            terminals: prepared.terminals,
        })
    }
    /// Advances the close in flight. Runs on every owner pass.
    pub(crate) fn pump_closing(&mut self) {
        let Some(mut closing) = self.closing.take() else {
            return;
        };
        let _cause = self.durability.enter(closing.cause);
        let result = match self.advance_close(&mut closing) {
            Ok(None) => {
                self.closing = Some(closing);
                return;
            }
            Ok(Some(snapshot)) => Ok(snapshot),
            Err(error) => self
                .host
                .cancel_close(closing.token)
                .map_err(Into::into)
                .and(Err(error)),
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
        #[cfg(test)]
        if closing.parked {
            return Ok(None);
        }
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
        self.begin_close(
            target,
            Box::new(move |result| {
                let _ = send.send(result);
            }),
        );
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
    pub(crate) fn close_request(
        &mut self,
        target: CloseTarget,
        admitted: Result<()>,
        done: CloseDone,
    ) {
        if let Err(error) = admitted.and_then(|()| self.assert_accepting()) {
            return done(Err(error));
        }
        self.begin_close(target, done);
    }
    /// `close_request` whose commit runs through `gate`.
    pub(crate) fn close_request_gated(
        &mut self,
        target: CloseTarget,
        gate: CommitGate,
        done: CloseDone,
    ) {
        if let Err(error) = self.assert_accepting() {
            return done(Err(error));
        }
        self.begin_close_gated(target, Some(gate), done);
    }
    /// Reads and terminal I/O on other tabs may run during a close. They can
    /// overtake their own client's earlier deferred requests; clients await
    /// each call, so they don't depend on that order.
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
            Status
            | SessionRecords
            | ReadWindowState
            | ReadTabActivity
            | ChangedSince { .. }
            | TabStats => true,
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
    /// A close in flight that never advances until `unpark_close`.
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
            cause: None,
            parked: true,
        });
        self.metadata_closing = true;
        outcome
    }
    /// Lets a parked close finish on the next pass.
    pub(crate) fn unpark_close(&mut self) {
        if let Some(closing) = &mut self.closing {
            closing.parked = false;
        }
    }
}

#[cfg(test)]
mod tests;
