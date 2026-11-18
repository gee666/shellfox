use super::*;
#[cfg(all(test, feature = "test-support"))]
#[path = "raw_process_launch_custody_tests.rs"]
mod tests;

impl RawProcess {
    /// Facts-only binding for the reviewed standard raw supervisor factory.
    /// The host supplies its trusted supervisor, never a request-selected helper.
    pub fn spawn_with_launch_binding(
        supervisor: &Path,
        command: RawCommand,
        binding: crate::launch_privilege::NativeLaunchBinding,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn_inner(supervisor, command, Some(binding), false, false, false)
    }
    /// Initial-launch snapshot only. It contains no retained Job/token handles.
    pub fn take_launch_evidence(
        &mut self,
    ) -> Option<crate::launch_privilege::NativeLaunchEvidence> {
        self.launch_evidence.take()
    }
    #[cfg(feature = "test-support")]
    pub fn spawn_fault_with_launch_binding(
        supervisor: &Path,
        command: RawCommand,
        binding: crate::launch_privilege::NativeLaunchBinding,
        deny_assignment: bool,
        skip_release: bool,
        fail_cleanup: bool,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn_inner(
            supervisor,
            command,
            Some(binding),
            deny_assignment,
            skip_release,
            fail_cleanup,
        )
    }
}
