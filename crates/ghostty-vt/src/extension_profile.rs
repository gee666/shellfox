use super::*;
impl VerifiedNativeExtension {
    /// Explicit constructor-only Native selection. Legacy remains unavailable.
    /// The resulting existing Terminal is passive: no PTY, clipboard, bell or
    /// metadata callbacks are registered. It grants no host/control capability.
    pub fn new_native_terminal(
        &self,
        cols: u16,
        rows: u16,
        profile: TerminalProfile,
        configuration: RestorationConfig,
        continuation_limit: usize,
    ) -> ExtensionResult<Terminal> {
        Terminal::from_extension_engine(self.inner.new_native_engine(
            cols,
            rows,
            profile,
            configuration,
            continuation_limit,
        )?)
    }
}
impl Terminal {
    /// Read-only canonical State query; never a replacement for atomic encoding.
    pub fn native_profile_state(
        &mut self,
        verified: &VerifiedNativeExtension,
    ) -> ExtensionResult<ProfileState> {
        verified.inner.profile_state(&mut self.engine)
    }
}
