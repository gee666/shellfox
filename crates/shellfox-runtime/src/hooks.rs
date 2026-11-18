//! Dependency-inverted live source observation. No activity policy lives here.
use crate::{Identity, InputKind, RequestId};
use std::{
    panic::{catch_unwind, RefUnwindSafe},
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveOutputFailure;
/// Runs synchronously on the owner thread AFTER successful live parsing/replies
/// and journal append. It must be bounded, non-reentrant, do no I/O/Host calls,
/// and retain no raw bytes. Service owns the shared coarse activity policy.
pub trait LiveOutputSink: Send + Sync + RefUnwindSafe {
    /// Pre-write echo observation only. Caller admission context is looked up by
    /// request ID, never recaptured here. This is NOT proof of accepted input.
    /// Zero encodings, failed length validation and parser replies bypass it.
    fn before_input(
        &self,
        _identity: Identity,
        _request_id: RequestId,
        _kind: InputKind,
        _encoded: &[u8],
        _at: Instant,
    ) -> Result<(), LiveOutputFailure> {
        Ok(())
    }
    fn output(
        &self,
        identity: Identity,
        sequence: u64,
        bytes: &[u8],
        at: Instant,
    ) -> Result<(), LiveOutputFailure>;
}
#[derive(Default)]
pub struct RuntimeHooks {
    pub live_output: Option<Arc<dyn LiveOutputSink>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LiveOutputStatus {
    Disabled = 0,
    Active = 1,
    Unavailable = 2,
}
impl RuntimeHooks {
    pub(crate) fn status(&self) -> LiveOutputStatus {
        if self.live_output.is_some() {
            LiveOutputStatus::Active
        } else {
            LiveOutputStatus::Disabled
        }
    }
}
pub(crate) struct LiveOutput {
    sink: Option<Arc<dyn LiveOutputSink>>,
}
impl LiveOutput {
    pub(crate) fn new(hooks: RuntimeHooks) -> Self {
        Self {
            sink: hooks.live_output,
        }
    }
    pub(crate) fn before_input(
        &mut self,
        identity: Identity,
        request_id: RequestId,
        kind: InputKind,
        encoded: &[u8],
        status: &AtomicU8,
    ) {
        let Some(sink) = &self.sink else {
            return;
        };
        let result =
            catch_unwind(|| sink.before_input(identity, request_id, kind, encoded, Instant::now()));
        if !matches!(result, Ok(Ok(()))) {
            self.sink = None;
            status.store(LiveOutputStatus::Unavailable as u8, Ordering::Release);
        }
    }
    /// No credit, frame, replay or observer state participates in this path.
    /// Supplier failure disables only this observer, never the real PTY owner.
    pub(crate) fn emit(
        &mut self,
        identity: Identity,
        sequence: u64,
        bytes: &[u8],
        status: &AtomicU8,
    ) {
        let Some(sink) = &self.sink else {
            return;
        };
        let result = catch_unwind(|| sink.output(identity, sequence, bytes, Instant::now()));
        if !matches!(result, Ok(Ok(()))) {
            self.sink = None;
            status.store(LiveOutputStatus::Unavailable as u8, Ordering::Release);
        }
    }
}
pub(crate) fn status(value: &AtomicU8) -> LiveOutputStatus {
    match value.load(Ordering::Acquire) {
        0 => LiveOutputStatus::Disabled,
        1 => LiveOutputStatus::Active,
        _ => LiveOutputStatus::Unavailable,
    }
}
