use super::*;
/// Accepted Native profile ANSI bytes, not universal/Legacy FullState. The
/// exported continuation may be incomplete UTF-8 and remains the final suffix.
#[derive(Debug)]
pub struct NativeAnsi {
    inner: native::NativeAnsi,
}
impl NativeAnsi {
    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }
    pub fn dimensions(&self) -> (u16, u16) {
        self.inner.dimensions()
    }
    /// Always constructs a fresh configured passive target internally. There is
    /// deliberately no API that applies the bytes over an arbitrary live view.
    pub fn restore(&self, verified: &VerifiedNativeExtension) -> ExtensionResult<Terminal> {
        Terminal::from_extension_engine(self.inner.restore(&verified.inner)?)
    }
}
impl Terminal {
    /// Matching canonical reset defaults/config are preflighted by the verified
    /// extension. Unsupported states return reason bits, never a lossy success.
    pub fn export_native_ansi_fresh(
        &mut self,
        verified: &VerifiedNativeExtension,
        limits: AnsiLimits,
        configuration: RestorationConfig,
    ) -> ExtensionResult<NativeAnsi> {
        verified
            .inner
            .export_native_ansi(&mut self.engine, limits, configuration)
            .map(|inner| NativeAnsi { inner })
    }
}
