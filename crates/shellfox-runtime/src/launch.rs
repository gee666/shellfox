//! Exact Runtime-generation binding for sealed native factory evidence.
use crate::*;
use shellfox_terminal::launch_privilege::{
    LaunchBindingVerifier, LaunchPrivilegeFacts, NativeLaunchBinding, NativeLaunchEvidence,
};
pub(crate) struct LaunchState {
    verifier: LaunchBindingVerifier,
    purpose: LaunchFactoryPurpose,
    evidence: Mutex<Option<NativeLaunchEvidence>>,
}
pub(crate) fn new(purpose: LaunchFactoryPurpose) -> (Arc<LaunchState>, NativeLaunchBinding) {
    let (binding, verifier) = shellfox_terminal::launch_privilege::new_launch_binding();
    (
        Arc::new(LaunchState {
            verifier,
            purpose,
            evidence: Mutex::new(None),
        }),
        binding,
    )
}
impl LaunchState {
    pub(crate) fn bind(&self, evidence: NativeLaunchEvidence) -> Result<(), RuntimeError> {
        if !self.verifier.matches(&evidence) || evidence.facts().purpose != self.purpose {
            return Err(RuntimeError::validation(
                "Native launch evidence belongs to another Runtime generation.",
            ));
        }
        let mut current = self
            .evidence
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if current.is_some() {
            return Err(RuntimeError::validation(
                "Native launch evidence is already bound.",
            ));
        }
        *current = Some(evidence);
        Ok(())
    }
    pub(crate) fn facts(&self) -> LaunchPrivilegeFacts {
        self.evidence
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map_or_else(LaunchPrivilegeFacts::default, NativeLaunchEvidence::facts)
    }
}
impl Runtime {
    /// Descriptive initial-launch facts only, never current peer/share permission
    /// or a promise about future in-session descendants/SSH/container contexts.
    pub fn launch_privilege_facts(&self) -> LaunchPrivilegeFacts {
        self.launch.facts()
    }
}
