//! Publication-gated live activity for the one raw automation presentation.
use crate::activity_integration::{InspectionScope, LiveActivitySink, OwnerActivity};
use anyhow::{ensure, Context, Result};
use shellfox_runtime::{Identity, LiveOutputFailure, LiveOutputSink, RuntimeHooks};
use std::{
    ffi::OsStr,
    sync::{Arc, Mutex},
    time::Instant,
};

struct State {
    sink: Option<Arc<LiveActivitySink>>,
    exit_seen: bool,
    exit_touch: Option<Instant>,
}
/// Created at Run.start and installed into its ORIGINAL external Runtime. Until
/// successful metadata publication it stores no bytes, timestamps or touches.
pub(super) struct AutomationActivity {
    identity: Identity,
    state: Mutex<State>,
}
impl AutomationActivity {
    pub(super) fn new(identity: Identity) -> Self {
        Self {
            identity,
            state: Mutex::new(State {
                sink: None,
                exit_seen: false,
                exit_touch: None,
            }),
        }
    }
    pub(super) fn hooks(self: &Arc<Self>) -> RuntimeHooks {
        RuntimeHooks {
            live_output: Some(self.clone()),
        }
    }
    /// The owner actor calls this around Host.publish_reserved_session itself,
    /// not around later focus/UI work. Holding the gate across commit prevents
    /// post-publication callbacks from being dropped or reaching a missing row.
    /// On failure no binding is installed and a newly admitted watch is removed.
    pub(super) fn publish<T>(
        &self,
        owner: &mut OwnerActivity,
        program: &OsStr,
        publication: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Automation activity gate unavailable."))?;
        let previous = owner.live_output_sink(self.identity);
        if let Some(bound) = &state.sink {
            ensure!(
                previous
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(bound, current)),
                "Published automation activity book was removed or replaced."
            );
        }
        watch_program(owner, self.identity, program, InspectionScope::Unsupported)?;
        let sink = owner
            .live_output_sink(self.identity)
            .context("Automation activity book missing.")?;
        if let Some(bound) = &state.sink {
            ensure!(
                Arc::ptr_eq(bound, &sink),
                "Published automation activity book changed."
            );
        }
        match publication() {
            Ok(receipt) => {
                state.sink = Some(sink);
                if state.exit_seen {
                    // The original raw owner observed this exit BEFORE Show.
                    // Seed policy state, deliberately discard its historical touch.
                    let _ = owner.confirmed_exit(self.identity);
                }
                Ok(receipt)
            }
            Err(error) => {
                if previous.is_none() {
                    owner.forget(self.identity);
                }
                Err(error)
            }
        }
    }
    /// Called only by the serialized ORIGINAL raw owner with actual normal raw
    /// exit or failed-cleanup evidence. Phase/relay/source Finish alone must NOT
    /// call this. This records activity, never grants process cleanup authority.
    /// Observe a pre-Show exit before publish, so late Show cannot replay a touch.
    pub(super) fn raw_exit_observed(&self, at: Instant) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Automation activity gate unavailable."))?;
        if state.exit_seen {
            return Ok(());
        }
        state.exit_seen = true;
        if let Some(sink) = &state.sink {
            let _ = sink.stop();
            state.exit_touch = Some(at);
        }
        Ok(())
    }
    /// Owner persistence converts through its existing clock/UTC anchor and
    /// touches updatedAt. Runtime callbacks never call Host or SQLite.
    pub(super) fn take_exit_touch(&self) -> Option<Instant> {
        self.state.lock().ok()?.exit_touch.take()
    }
}
impl LiveOutputSink for AutomationActivity {
    fn output(
        &self,
        identity: Identity,
        sequence: u64,
        bytes: &[u8],
        at: Instant,
    ) -> std::result::Result<(), LiveOutputFailure> {
        if identity != self.identity {
            return Err(LiveOutputFailure);
        }
        let state = self.state.lock().map_err(|poisoned| {
            if let Some(sink) = &poisoned.into_inner().sink {
                sink.disable();
            }
            LiveOutputFailure
        })?;
        if let Some(sink) = &state.sink {
            sink.output(identity, sequence, bytes, at)?;
        }
        Ok(())
    }
    // Raw stdin remains AutomationRuns/RawProcess ownership. No before_input
    // delegation, PTY receipt or settlement context is invented by this gate.
}
/// Program identity is inspection configuration only, never shell creation
/// provenance. Unrepresentable OS strings disable inspection, not live activity.
pub(super) fn watch_program(
    owner: &mut OwnerActivity,
    identity: Identity,
    program: &OsStr,
    scope: InspectionScope,
) -> Result<()> {
    let (expected, scope) = match program.to_str().filter(|text| !text.is_empty()) {
        Some(program) => (program.to_owned(), scope),
        None => (
            "uninspectable-program-encoding".to_owned(),
            InspectionScope::Unsupported,
        ),
    };
    let watched = owner.watch(identity, expected, scope);
    let watched = match watched {
        Err(crate::activity::InspectionFailure::Malformed) => owner.watch(
            identity,
            "unsupported-program-inspection".to_owned(),
            InspectionScope::Unsupported,
        ),
        result => result,
    };
    watched.map_err(|_| anyhow::anyhow!("Program activity watch unavailable."))
}
