use super::*;
/// Owned Native002 raw-v1 artifact plus its explicit cut State and configuration.
/// Imported bodies are untrusted until every decode performs FINISH/EOF checks.
/// Never label this generic FullState or send its bytes to legacy ANSI clients.
#[derive(Debug)]
pub struct NativeSnapshot {
    inner: native::NativeSnapshot,
}
impl NativeSnapshot {
    pub fn from_untrusted(
        verified: &VerifiedNativeExtension,
        bytes: &[u8],
        metadata: SnapshotMetadata,
        limits: SnapshotLimits,
    ) -> ExtensionResult<Self> {
        native::NativeSnapshot::from_untrusted(verified.inner, bytes, metadata, limits)
            .map(|inner| Self { inner })
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }
    pub fn metadata(&self) -> &SnapshotMetadata {
        self.inner.metadata()
    }
    /// Fresh owned result only. No old terminal is mutated/replaced on failure.
    /// Decoded parser/default/continuation state is adopted, not replayed/reset.
    pub fn decode(
        &self,
        verified: &VerifiedNativeExtension,
        limits: SnapshotLimits,
    ) -> ExtensionResult<(Terminal, SnapshotReport)> {
        let (engine, report) = verified.inner.decode_native_snapshot(&self.inner, limits)?;
        Ok((Terminal::from_extension_engine(engine)?, report))
    }
}
/// Explicit existing Native002 / Native0 raw-v1 compatibility, not authority.
#[derive(Debug, Clone, Copy)]
pub struct Native0RawV1Compatibility {
    inner: native::Native0RawV1Compatibility,
}
impl Native0RawV1Compatibility {
    /// Caller binds this independently expected sender through authenticated
    /// negotiation and the recovery message's original metadata/body/cut refs.
    /// Raw-v1 has no embedded identity; this does not authenticate a binary.
    pub fn for_negotiated_sender(
        receiver: &VerifiedNativeExtension,
        expected_sender: EngineIdentity,
    ) -> Self {
        Self {
            inner: native::Native0RawV1Compatibility::for_negotiated_sender(
                receiver.inner,
                expected_sender,
            ),
        }
    }
}
/// Unchanged original sender metadata and bounded bytes, distinct from the strict
/// same-build NativeSnapshot. Every decode returns a fresh passive target only.
#[derive(Debug)]
pub struct CrossTargetNative0RawV1Snapshot {
    inner: native::CrossTargetNative0RawV1Snapshot,
}
impl CrossTargetNative0RawV1Snapshot {
    /// Never derive the expected sender by blindly trusting the supplied metadata.
    /// Unsupported/profile/configuration/budget failures publish no replacement.
    pub fn from_untrusted(
        compatibility: Native0RawV1Compatibility,
        bytes: &[u8],
        metadata: SnapshotMetadata,
        limits: SnapshotLimits,
    ) -> ExtensionResult<Self> {
        native::CrossTargetNative0RawV1Snapshot::from_untrusted(
            compatibility.inner,
            bytes,
            metadata,
            limits,
        )
        .map(|inner| Self { inner })
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }
    pub fn metadata(&self) -> &SnapshotMetadata {
        self.inner.metadata()
    }
    pub fn decode(
        &self,
        receiver: &VerifiedNativeExtension,
        limits: SnapshotLimits,
    ) -> ExtensionResult<(Terminal, SnapshotReport)> {
        let (engine, report) = receiver
            .inner
            .decode_cross_target_native0_raw_v1_snapshot(&self.inner, limits)?;
        Ok((Terminal::from_extension_engine(engine)?, report))
    }
}
impl Terminal {
    /// Exclusive &mut borrow spans canonical preflight and atomic State/body
    /// capture. Source callbacks, pending queries and metadata are not drained.
    pub fn encode_native_snapshot(
        &mut self,
        verified: &VerifiedNativeExtension,
        limits: SnapshotLimits,
        configuration: RestorationConfig,
    ) -> ExtensionResult<(NativeSnapshot, SnapshotReport)> {
        let (inner, report) =
            verified
                .inner
                .encode_native_snapshot(&mut self.engine, limits, configuration)?;
        Ok((NativeSnapshot { inner }, report))
    }
}
