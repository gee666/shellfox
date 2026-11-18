//! Explicit Native002 / Native0 raw-v1 admission, not build authentication.
use super::*;

/// Selects ONLY the existing Native002 ABI / Native0 raw-v1 codec semantics.
/// The caller must bind `expected_sender` through authenticated negotiation and
/// bind that original identity, metadata and body to the same recovery message.
/// Raw-v1 has NO embedded engine identity. This is claimed source/message binding,
/// not binary authenticity, a transport authenticator or a control capability.
#[derive(Debug, Clone, Copy)]
pub struct Native0RawV1Compatibility {
    receiver: VerifiedNativeExtension,
    expected_sender: EngineIdentity,
}
impl Native0RawV1Compatibility {
    pub fn for_negotiated_sender(
        receiver: VerifiedNativeExtension,
        expected_sender: EngineIdentity,
    ) -> Self {
        Self {
            receiver,
            expected_sender,
        }
    }
    pub(super) fn validate(
        &self,
        receiver: VerifiedNativeExtension,
        metadata: &SnapshotMetadata,
    ) -> Result<ProfileState> {
        if receiver.identity != self.receiver.identity
            || metadata.engine_identity != self.expected_sender
        {
            return Err(Error::IdentityMismatch);
        }
        metadata.validate_native0_raw_v1()
    }
}

/// Separate from strict NativeSnapshot: original sender metadata/body are owned
/// unchanged. Admission does not certify decode success or cross-platform support.
#[derive(Debug)]
pub struct CrossTargetNative0RawV1Snapshot {
    snapshot: NativeSnapshot,
    compatibility: Native0RawV1Compatibility,
}
impl CrossTargetNative0RawV1Snapshot {
    /// `compatibility` must come from the independently negotiated sender, NOT
    /// from blindly copying this untrusted metadata's identity. No defaults are
    /// substituted for the source's explicit cut State/restoration configuration.
    pub fn from_untrusted(
        compatibility: Native0RawV1Compatibility,
        bytes: &[u8],
        metadata: SnapshotMetadata,
        limits: SnapshotLimits,
    ) -> Result<Self> {
        limits.validate()?;
        compatibility.validate(compatibility.receiver, &metadata)?;
        let snapshot = NativeSnapshot::copy_untrusted(bytes, metadata, limits)?;
        Ok(Self {
            snapshot,
            compatibility,
        })
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.snapshot.as_bytes()
    }
    pub fn metadata(&self) -> &SnapshotMetadata {
        self.snapshot.metadata()
    }
}
impl VerifiedNativeExtension {
    /// Fresh passive result only after the existing bounded C decoder's profile,
    /// configuration, State, FINISH/EOF, report and adoption checks. The C options
    /// still carry THIS receiver's verified LOCAL build identity, never the sender's.
    /// No old target is passed, reset, replaced or destroyed on failure.
    pub fn decode_cross_target_native0_raw_v1_snapshot(
        &self,
        snapshot: &CrossTargetNative0RawV1Snapshot,
        limits: SnapshotLimits,
    ) -> Result<(Engine, SnapshotReport)> {
        limits.validate()?;
        let expected = snapshot
            .compatibility
            .validate(*self, snapshot.metadata())?;
        self.decode_snapshot_body(&snapshot.snapshot, expected, limits)
    }
}
