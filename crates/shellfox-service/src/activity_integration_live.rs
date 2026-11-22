//! One bounded filter per owned generation. No Host calls, reentrancy or VT history.
use super::input::InputContexts;
use super::{AcceptedInputContext, ActivityClock, InputAdmission, InputContextFailure};
use shellfox_runtime::{InputKind, RequestId, Response, RuntimeError};
#[cfg(test)]
#[path = "activity_integration_live_tests.rs"]
mod tests;
use crate::activity::TerminalActivity;
use shellfox_runtime::Identity;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityUnavailable;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveActivityUpdate {
    pub identity: Identity,
    pub last_sequence: u64,
    /// ALL live data touches pending TTL, matching Electron service.ts:189.
    pub output_at_ms: Option<u64>,
    /// Separate busy-policy metric, never a replacement for the TTL touch above.
    pub meaningful_at_ms: Option<u64>,
    pub busy_changed: Option<bool>,
    pub available: bool,
}
struct State {
    activity: TerminalActivity,
    sequence: u64,
    output_at_ms: Option<u64>,
    meaningful_at_ms: Option<u64>,
    reported_busy: bool,
    changed: bool,
}
/// The service retains this same object for before-input/tick/stop. Runtime only
/// borrows bytes during the callback; the existing filter keeps at most an
/// 18-byte partial idle escape, never terminal text/history or per-view copies.
pub struct LiveActivitySink {
    identity: Identity,
    clock: Arc<ActivityClock>,
    state: Mutex<State>,
    inputs: Mutex<InputContexts>,
    available: AtomicBool,
    unavailable_pending: AtomicBool,
}
impl LiveActivitySink {
    pub fn new(identity: Identity, clock: Arc<ActivityClock>) -> Self {
        Self {
            identity,
            clock,
            state: Mutex::new(State {
                activity: TerminalActivity::new(),
                sequence: 0,
                output_at_ms: None,
                meaningful_at_ms: None,
                reported_busy: false,
                changed: false,
            }),
            inputs: Mutex::new(InputContexts::default()),
            available: AtomicBool::new(true),
            unavailable_pending: AtomicBool::new(false),
        }
    }
    fn unavailable(&self) -> ActivityUnavailable {
        if self.available.swap(false, Ordering::AcqRel) {
            self.unavailable_pending.store(true, Ordering::Release);
        }
        ActivityUnavailable
    }
    fn with_state<T>(
        &self,
        action: impl FnOnce(&mut State) -> T,
    ) -> Result<T, ActivityUnavailable> {
        if !self.available.load(Ordering::Acquire) {
            return Err(ActivityUnavailable);
        }
        let mut state = self.state.lock().map_err(|_| self.unavailable())?;
        Ok(action(&mut state))
    }
    /// Only the authoritative live feed calls this, after successful parser feed.
    /// No callback is permitted for replay, rendering, recovery or observer copies.
    pub fn observe(
        &self,
        identity: Identity,
        sequence: u64,
        bytes: &[u8],
        at: Instant,
    ) -> Result<(), ActivityUnavailable> {
        if identity != self.identity {
            return Err(self.unavailable());
        }
        let now_ms = self.clock.at(at).ok_or_else(|| self.unavailable())?;
        self.with_state(|state| {
            if sequence <= state.sequence || bytes.is_empty() {
                return Err(self.unavailable());
            }
            state.sequence = sequence;
            let output = state.activity.output_activity(bytes, now_ms);
            state.output_at_ms = Some(
                state
                    .output_at_ms
                    .map_or(now_ms, |previous| previous.max(now_ms)),
            );
            if let Some(at) = output.meaningful_at_ms {
                state.meaningful_at_ms = Some(
                    state
                        .meaningful_at_ms
                        .map_or(at, |previous| previous.max(at)),
                );
            }
            state.changed = true;
            Ok(())
        })?
    }
    pub fn register_input(
        &self,
        identity: Identity,
        request: RequestId,
        admission: InputAdmission,
    ) -> Result<(), InputContextFailure> {
        if identity != self.identity {
            return Err(InputContextFailure::Stale);
        }
        let mut inputs = self.inputs.lock().map_err(|_| {
            self.unavailable();
            InputContextFailure::Unavailable
        })?;
        inputs.register(request, admission)
    }
    pub fn finish_input(
        &self,
        identity: Identity,
        request: RequestId,
        result: &Result<Response, RuntimeError>,
    ) -> Result<Option<AcceptedInputContext>, InputContextFailure> {
        if identity != self.identity {
            return Err(InputContextFailure::Stale);
        }
        let mut inputs = self.inputs.lock().map_err(|_| {
            self.unavailable();
            InputContextFailure::Unavailable
        })?;
        let finished = match result {
            Ok(Response::Input(receipt)) => inputs.finish(identity, request, Some(*receipt)),
            Err(_) => {
                inputs.cancel(request);
                Ok(None)
            }
            Ok(_) => {
                if inputs.cancel(request) {
                    Err(InputContextFailure::InvalidReceipt)
                } else {
                    Ok(None)
                }
            }
        };
        if finished.is_err() {
            self.unavailable();
        }
        finished
    }
    pub fn cancel_input(&self, identity: Identity, request: RequestId) -> bool {
        if identity != self.identity {
            return false;
        }
        match self.inputs.lock() {
            Ok(mut inputs) => inputs.cancel(request),
            Err(poisoned) => {
                self.unavailable();
                poisoned.into_inner().cancel(request)
            }
        }
    }
    pub fn cancel_all_inputs(&self, identity: Identity) -> usize {
        if identity != self.identity {
            return 0;
        }
        match self.inputs.lock() {
            Ok(mut inputs) => inputs.clear(),
            Err(poisoned) => {
                self.unavailable();
                poisoned.into_inner().clear()
            }
        }
    }
    fn pre_write(
        &self,
        identity: Identity,
        request: RequestId,
        kind: InputKind,
        encoded: &[u8],
        at: Instant,
    ) -> Result<(), ActivityUnavailable> {
        if identity != self.identity {
            return Err(self.unavailable());
        }
        let now_ms = self.clock.at(at).ok_or_else(|| self.unavailable())?;
        // One admission book, with a separate mutex so optional filter poison
        // cannot destroy actor-owned settlement context. Never hold both locks.
        let meaningful = {
            let mut inputs = self.inputs.lock().map_err(|_| self.unavailable())?;
            inputs
                .prepare(request, kind, encoded)
                .map_err(|_| self.unavailable())?
        };
        self.with_state(|state| {
            if meaningful {
                state.activity.input(now_ms);
            }
        })
    }
    pub fn tick(&self, now_ms: u64) -> Result<Option<bool>, ActivityUnavailable> {
        self.with_state(|state| {
            let transition = state.activity.tick(now_ms);
            state.changed |= transition.is_some();
            transition
        })
    }
    pub fn quiet_at(&self) -> Option<u64> {
        self.with_state(|state| state.activity.quiet_at()).ok().flatten()
    }
    pub fn stop(&self) -> Result<Option<bool>, ActivityUnavailable> {
        self.with_state(|state| {
            let transition = state.activity.stop();
            state.changed |= transition.is_some();
            transition
        })
    }
    pub fn disable(&self) {
        self.unavailable();
    }
    pub fn last_meaningful_output_ms(&self) -> Option<u64> {
        self.with_state(|state| state.activity.last_meaningful_output_ms())
            .ok()
            .flatten()
    }
    pub fn last_sequence(&self) -> Option<u64> {
        self.with_state(|state| state.sequence).ok()
    }
    pub fn status(&self) -> (bool, bool) {
        self.with_state(|state| state.activity.is_busy())
            .map_or((false, false), |busy| (busy, true))
    }
    /// Coalesced metadata only. A busy transition does not itself touch inactivity.
    pub fn take_update(&self) -> Option<LiveActivityUpdate> {
        if self.unavailable_pending.swap(false, Ordering::AcqRel) {
            return Some(LiveActivityUpdate {
                identity: self.identity,
                last_sequence: 0,
                output_at_ms: None,
                meaningful_at_ms: None,
                busy_changed: None,
                available: false,
            });
        }
        self.with_state(|state| {
            if !state.changed {
                return None;
            }
            state.changed = false;
            let busy = state.activity.is_busy();
            let busy_changed = (state.reported_busy != busy).then_some(busy);
            state.reported_busy = busy;
            Some(LiveActivityUpdate {
                identity: self.identity,
                last_sequence: state.sequence,
                output_at_ms: state.output_at_ms.take(),
                meaningful_at_ms: state.meaningful_at_ms.take(),
                busy_changed,
                available: true,
            })
        })
        .ok()
        .flatten()
    }
}
impl shellfox_runtime::LiveOutputSink for LiveActivitySink {
    fn before_input(
        &self,
        identity: Identity,
        request: RequestId,
        kind: InputKind,
        encoded: &[u8],
        at: Instant,
    ) -> Result<(), shellfox_runtime::LiveOutputFailure> {
        self.pre_write(identity, request, kind, encoded, at)
            .map_err(|_| shellfox_runtime::LiveOutputFailure)
    }
    fn output(
        &self,
        identity: Identity,
        sequence: u64,
        bytes: &[u8],
        at: Instant,
    ) -> Result<(), shellfox_runtime::LiveOutputFailure> {
        self.observe(identity, sequence, bytes, at)
            .map_err(|_| shellfox_runtime::LiveOutputFailure)
    }
}
