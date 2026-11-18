//! A bounded authentication matcher alongside the ORIGINAL activity/input sink.
use crate::activity_integration::LiveActivitySink;
use shellfox_runtime::{Identity, InputKind, LiveOutputFailure, LiveOutputSink, RequestId};
use shellfox_ssh::system::AuthenticationProof;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

pub(super) struct ProgramTap {
    identity: Identity,
    activity: Arc<LiveActivitySink>,
    proof: Mutex<AuthenticationProof>,
    reported: AtomicBool,
    diagnostics: Mutex<super::diagnostics::PublicDiagnostics>,
}
impl ProgramTap {
    pub fn new(
        identity: Identity,
        activity: Arc<LiveActivitySink>,
        proof: AuthenticationProof,
    ) -> Self {
        Self {
            identity,
            activity,
            proof: Mutex::new(proof),
            reported: AtomicBool::new(false),
            diagnostics: Mutex::new(Default::default()),
        }
    }
    pub fn public_diagnostics(&self) -> Option<String> {
        self.diagnostics.lock().ok().map(|value| value.summary())
    }
    pub fn confirmation(&self) -> Option<AuthenticationProof> {
        if self.reported.load(Ordering::Acquire) {
            return None;
        }
        let proof = self.proof.lock().ok()?;
        if proof.confirmed() {
            self.reported.store(true, Ordering::Release);
            Some(proof.clone())
        } else {
            None
        }
    }
}
impl LiveOutputSink for ProgramTap {
    fn before_input(
        &self,
        identity: Identity,
        request: RequestId,
        kind: InputKind,
        encoded: &[u8],
        at: Instant,
    ) -> Result<(), LiveOutputFailure> {
        if identity != self.identity {
            return Err(LiveOutputFailure);
        }
        if self
            .activity
            .before_input(identity, request, kind, encoded, at)
            .is_err()
        {
            self.activity.disable();
        }
        // Activity failure remains explicit on its own sink. Keep bounded auth proof
        // observation alive; neither observer is allowed to kill the actual PTY.
        Ok(())
    }
    fn output(
        &self,
        identity: Identity,
        sequence: u64,
        bytes: &[u8],
        at: Instant,
    ) -> Result<(), LiveOutputFailure> {
        if identity != self.identity {
            return Err(LiveOutputFailure);
        }
        if let Ok(mut proof) = self.proof.lock() {
            if !proof.confirmed() {
                if let Ok(mut diagnostics) = self.diagnostics.lock() {
                    diagnostics.observe(bytes);
                }
            }
            proof.observe(bytes);
        }
        if self.activity.output(identity, sequence, bytes, at).is_err() {
            self.activity.disable();
        }
        Ok(())
    }
}
