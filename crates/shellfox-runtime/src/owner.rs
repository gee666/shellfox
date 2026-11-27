use crate::{delivery::Delivery, observers::ObserverHub, owner_wake::EventSender, *};
#[cfg(test)]
#[path = "controller_tests.rs"]
mod controller_tests;
#[cfg(test)]
#[path = "owner_event_tests.rs"]
mod event_tests;
#[cfg(test)]
#[path = "owner_recovery_audit_tests.rs"]
mod recovery_audit_tests;
#[cfg(test)]
#[path = "owner_test_support.rs"]
mod test_support;
use shellfox_ghostty_vt::Terminal;
use shellfox_terminal::{PtyConfig, PtySession};
use std::{
    collections::VecDeque,
    sync::mpsc::{Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};

/// Cleanup retry after a failed teardown.
const CLEANUP_RETRY: Duration = Duration::from_millis(100);
/// Poll cadence for teardown the platform can't signal (see Owner::deadline).
const POLL_INTERVAL: Duration = Duration::from_millis(2);
/// Recheck during woken teardown: Windows doesn't guarantee delivery of the
/// Job completion-port messages behind those wakes.
const TEARDOWN_RECHECK: Duration = Duration::from_millis(250);

#[cfg(windows)]
#[path = "elevated_pty.rs"]
mod elevated_pty;
#[path = "external_start.rs"]
mod external_start;
pub(crate) use external_start::{run as run_external, ExternalStartServices};
pub(crate) struct OwnerServices {
    pub inspection: Option<inspection::InspectionPublisher>,
    pub hooks: RuntimeHooks,
    #[cfg(windows)]
    pub administrator: Option<shellfox_terminal::elevated::InteractiveHelper>,
    pub launch: Arc<launch::LaunchState>,
    pub launch_binding: shellfox_terminal::NativeLaunchBinding,
}
pub(crate) struct Owner {
    pub identity: Identity,
    pub limits: GridLimits,
    pub terminal: Terminal,
    pub presentations: presentation::Presentations,
    pub layouts: layout::Layouts,
    pub pty: Option<PtySession>,
    #[cfg(windows)]
    pub(crate) elevated: Option<elevated_pty::ElevatedPty>,
    pub source_kind: SourceKind,
    pub external: Option<external::ExternalOwner>,
    pub delivery: Delivery,
    pub observers: ObserverHub,
    pub observers_first: bool,
    pub phase: Phase,
    pub shared: Arc<Shared>,
    pub outgoing: VecDeque<Event>,
    pub dirty: bool,
    fatal: bool,
    pub(crate) exit_sent: bool,
    retry_at: Instant,
    /// Root exit of the owned PTY; None means no wake was registered and the
    /// owner polls the PTY instead.
    pub(crate) exit: Option<shellfox_terminal::ExitSignal>,
    inspection: Option<inspection::InspectionPublisher>,
    pub(crate) live_output: hooks::LiveOutput,
}

pub(crate) fn run(
    identity: Identity,
    config: PtyConfig,
    limits: GridLimits,
    incoming: Receiver<Request>,
    events: SyncSender<Event>,
    shared: Arc<Shared>,
    services: OwnerServices,
) {
    let events = EventSender::new(events, Arc::clone(&shared));
    let terminal = match Terminal::new(config.cols, config.rows).map_err(RuntimeError::unavailable)
    {
        Ok(terminal) => terminal,
        Err(error) => {
            *shared.phase.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Closed;
            let _ = events.try_send(Event::Started(Err(error)));
            events.finish_round();
            return;
        }
    };
    let mut owner = Owner {
        identity,
        limits,
        terminal,
        presentations: presentation::Presentations::new(),
        layouts: layout::Layouts::new(config.cols, config.rows),
        pty: None,
        #[cfg(windows)]
        elevated: None,
        source_kind: SourceKind::OwnedPty,
        external: None,
        delivery: Delivery::new(),
        observers: ObserverHub::new(),
        observers_first: false,
        phase: Phase::Starting,
        shared,
        outgoing: VecDeque::new(),
        dirty: true,
        fatal: false,
        exit_sent: false,
        retry_at: Instant::now(),
        exit: None,
        inspection: services.inspection,
        live_output: hooks::LiveOutput::new(services.hooks),
    };
    if owner.shared.abandoned.load(Ordering::Acquire) {
        return;
    }
    #[cfg(windows)]
    if let Some(factory) = services.administrator {
        owner.start_administrator(factory, &config);
        drive(owner, incoming, events);
        return;
    }
    let spawned = PtySession::spawn_bound(config, services.launch_binding);
    match spawned {
        Ok(mut pty) => {
            if let Some(evidence) = pty.take_launch_evidence() {
                let _ = services.launch.bind(evidence);
            }
            if let Some(inspection) = &owner.inspection {
                inspection.bind(pty.inspection_capability());
            }
            // Without a wake the owner falls back to polling; output is never lost.
            owner.exit = pty.set_wake(owner.shared.signal.waker()).ok();
            owner.pty = Some(pty);
            owner.set_phase(Phase::Active, None);
            owner.outgoing.push_front(Event::Started(Ok(())));
        }
        Err(error) => {
            owner.pty = error.take_session();
            if let Some(inspection) = &owner.inspection {
                inspection.bind(
                    owner
                        .pty
                        .as_ref()
                        .ok_or(InspectionFailure::Unavailable)
                        .and_then(PtySession::inspection_capability),
                );
            }
            let failure = RuntimeError::unavailable(error);
            owner.set_phase(
                if owner.pty.is_some() {
                    Phase::Draining
                } else {
                    Phase::Closed
                },
                Some(failure.message.clone()),
            );
            owner.fatal = owner.pty.is_some();
            owner.outgoing.push_front(Event::Started(Err(failure)));
        }
    }
    drive(owner, incoming, events);
}
fn drive(mut owner: Owner, incoming: Receiver<Request>, events: EventSender) {
    let signal = Arc::clone(&owner.shared.signal);
    loop {
        if owner.shared.abandoned.load(Ordering::Acquire) {
            // No consumer remains. Cancellation may discard output, never ownership.
            if owner.cleanup().is_ok() {
                return;
            }
            signal.wait(Some(Instant::now() + CLEANUP_RETRY));
            continue;
        }
        owner.flush(&events);
        let mut busy = false;
        if owner.fatal {
            if Instant::now() >= owner.retry_at {
                let _ = owner.cleanup();
                owner.retry_at = Instant::now() + CLEANUP_RETRY;
            }
        } else {
            busy |= owner.poll_output(16);
        }
        if owner.outgoing.len() < 2 {
            match incoming.try_recv() {
                Ok(request) => {
                    busy = true;
                    let result = request
                        .command
                        .validate(owner.limits)
                        .and_then(|_| owner.command(request.id, request.command));
                    if let Err(error) = &result {
                        if error.code == ErrorCode::Unavailable {
                            owner.fail(error.clone());
                        }
                    }
                    owner.outgoing.push_back(Event::Reply {
                        id: request.id,
                        result,
                    });
                }
                Err(TryRecvError::Disconnected) => {
                    owner.shared.abandoned.store(true, Ordering::Release);
                    busy = true;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if owner.dirty && owner.shared.legacy_requested.load(Ordering::Acquire) {
            if let Err(error) = owner.publish() {
                // Optional presentation failure stops the display subscription,
                // never destroys the authoritative parser or owned process.
                owner
                    .shared
                    .legacy_requested
                    .store(false, Ordering::Release);
                owner.set_phase(owner.phase, Some(error.message));
            }
        }
        owner.emit_events(&events);
        let mut round = events.finish_round();
        if round.full {
            // Ask for a wake when the consumer takes an event, then retry once:
            // it may have emptied the channel before seeing the request.
            owner.shared.needs_space.store(true, Ordering::Release);
            owner.flush(&events);
            owner.emit_events(&events);
            round = events.finish_round();
            if !round.full {
                owner.shared.needs_space.store(false, Ordering::Release);
            }
        }
        // A full channel waits for the consumer; otherwise keep going while
        // the round made progress, since reads and emission are budgeted.
        if !busy && (round.full || !round.sent) {
            signal.wait(owner.deadline());
        }
    }
}
impl Owner {
    /// The next time the owner must look without being woken: cleanup
    /// retries, and PTY teardown. Teardown polls where the platform has no
    /// wake for it (macOS, or a PTY whose wake failed to register), and is
    /// rechecked slowly where it does.
    fn deadline(&self) -> Option<Instant> {
        if self.shared.abandoned.load(Ordering::Acquire) {
            return Some(Instant::now());
        }
        if self.fatal {
            return Some(self.retry_at);
        }
        self.pty.as_ref()?;
        teardown_step(self.exit.as_ref(), self.phase == Phase::Draining)
            .map(|step| Instant::now() + step)
    }
}
/// How often to look at a PTY without a wake. None means wakes alone suffice.
fn teardown_step(exit: Option<&shellfox_terminal::ExitSignal>, draining: bool) -> Option<Duration> {
    let Some(exit) = exit else {
        return Some(POLL_INTERVAL);
    };
    (exit.exited() || draining).then(|| {
        if exit.wakes_teardown() {
            TEARDOWN_RECHECK
        } else {
            POLL_INTERVAL
        }
    })
}
#[cfg(test)]
#[test]
fn only_teardown_without_a_wake_polls_fast() {
    use shellfox_terminal::ExitSignal;
    let unwoken = ExitSignal::default();
    assert_eq!(teardown_step(None, false), Some(POLL_INTERVAL));
    assert_eq!(teardown_step(Some(&unwoken), false), None);
    assert_eq!(teardown_step(Some(&unwoken), true), Some(POLL_INTERVAL));
}
impl Owner {
    pub(crate) fn set_phase(&mut self, phase: Phase, error: Option<String>) {
        if self.phase == phase && error.is_none() {
            return;
        }
        if phase == Phase::Closed {
            if let Some(inspection) = &self.inspection {
                inspection.stop();
            }
        }
        self.phase = phase;
        *self.shared.phase.lock().unwrap_or_else(|e| e.into_inner()) = phase;
        // Status must stay bounded even if cleanup repeatedly fails.
        self.outgoing
            .retain(|event| !matches!(event, Event::Phase { .. }));
        self.outgoing.push_back(Event::Phase { phase, error });
    }
    pub(crate) fn active(&self) -> Result<(), RuntimeError> {
        if self.source_kind == SourceKind::ExternalBytes {
            return Err(RuntimeError {
                code: ErrorCode::Unsupported,
                message: "External byte sources are read-only views.".into(),
            });
        }
        if self.phase != Phase::Active {
            return Err(RuntimeError {
                code: ErrorCode::Closed,
                message: "terminal is not active".into(),
            });
        }
        Ok(())
    }
    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(), RuntimeError> {
        self.active()?;
        if bytes.is_empty() {
            return Ok(());
        }
        #[cfg(windows)]
        if let Some(pty) = &mut self.elevated {
            return pty.write(bytes);
        }
        self.pty
            .as_mut()
            .ok_or_else(|| RuntimeError::unavailable("missing PTY"))?
            .write(bytes)
            .map_err(|error| {
                // Backpressure is never Unavailable, which fails the terminal.
                if error.is::<shellfox_terminal::InputBusy>() {
                    RuntimeError {
                        code: ErrorCode::ResourceLimit,
                        message: error.to_string(),
                    }
                } else {
                    RuntimeError::unavailable(error)
                }
            })
    }
    pub(crate) fn replies(&mut self) -> Result<(), RuntimeError> {
        let replies = self
            .terminal
            .drain_replies()
            .map_err(RuntimeError::unavailable)?;
        // Final output is parsed for display, but a retired PTY receives no replies.
        if self.source_kind == SourceKind::OwnedPty && self.phase == Phase::Active {
            match self.write(&replies) {
                // A shell not reading its input, or an administrator helper
                // whose input closed as it exits, misses these query replies.
                // That must not fail the terminal.
                Err(error)
                    if matches!(error.code, ErrorCode::ResourceLimit | ErrorCode::Closed) => {}
                result => result?,
            }
        }
        Ok(())
    }
    pub(crate) fn publish(&mut self) -> Result<(), RuntimeError> {
        let Some(lease) = self.delivery.lease() else {
            return Ok(());
        };
        let screen = self.presentations.legacy(
            &mut self.terminal,
            self.identity,
            self.delivery.current_sequence(),
            self.limits,
            lease,
        )?;
        *self
            .shared
            .screen
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(screen);
        self.dirty = false;
        Ok(())
    }
    pub(crate) fn cleanup(&mut self) -> Result<(), RuntimeError> {
        if let Some(source) = &mut self.external {
            source.close(false, false);
        }
        if let Some(pty) = self.pty.as_mut() {
            pty.shutdown().map_err(RuntimeError::unavailable)?;
        }
        self.pty = None;
        #[cfg(windows)]
        if let Some(pty) = &mut self.elevated {
            pty.shutdown()?;
            self.elevated = None;
        }
        self.fatal = false;
        self.set_phase(Phase::Closed, None);
        Ok(())
    }
    pub(crate) fn fail(&mut self, error: RuntimeError) {
        if let Some(source) = &mut self.external {
            source.close(false, true);
        }
        self.fatal = true;
        // No repeated parser operations after a poisoned engine.
        self.dirty = false;
        self.set_phase(Phase::Draining, Some(error.message));
        if let Err(cleanup) = self.cleanup() {
            self.set_phase(
                Phase::Draining,
                Some(format!("cleanup retained: {cleanup}")),
            );
        }
    }
}
