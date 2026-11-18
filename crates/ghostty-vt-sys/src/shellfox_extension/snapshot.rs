use super::*;
use crate::Engine;
#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
#[path = "snapshot_types.rs"]
mod types;
pub use types::{ProfileState, SnapshotLimits, SnapshotMetadata, SnapshotReport};
#[path = "snapshot_compatibility.rs"]
mod compatibility;
pub use compatibility::{CrossTargetNative0RawV1Snapshot, Native0RawV1Compatibility};

/// Native002 raw-v1 artifact plus explicit configuration/State. This type is not
/// universal FullState and is not a proof that an imported body has decoded.
#[derive(Debug)]
pub struct NativeSnapshot {
    bytes: Vec<u8>,
    metadata: SnapshotMetadata,
}
impl NativeSnapshot {
    pub fn from_untrusted(
        verified: VerifiedNativeExtension,
        bytes: &[u8],
        metadata: SnapshotMetadata,
        limits: SnapshotLimits,
    ) -> Result<Self> {
        limits.validate()?;
        metadata.validate(verified)?;
        Self::copy_untrusted(bytes, metadata, limits)
    }
    fn copy_untrusted(
        bytes: &[u8],
        metadata: SnapshotMetadata,
        limits: SnapshotLimits,
    ) -> Result<Self> {
        let mut owned = allocation(bytes.len(), limits.max_encoded_bytes)?;
        owned.copy_from_slice(bytes);
        Ok(Self {
            bytes: owned,
            metadata,
        })
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn metadata(&self) -> &SnapshotMetadata {
        &self.metadata
    }
}
impl VerifiedNativeExtension {
    /// One exclusive source borrow, one bounded atomic C capture of State/body.
    /// No separate getter is used as the artifact's cut State.
    pub fn encode_native_snapshot(
        &self,
        engine: &mut Engine,
        limits: SnapshotLimits,
        config: RestorationConfig,
    ) -> Result<(NativeSnapshot, SnapshotReport)> {
        limits.validate()?;
        config.validate()?;
        let raw = terminal::borrow(engine)?;
        let options = EncodeOptions::build(self, limits, config);
        let mut bytes = allocation(limits.max_encoded_bytes, 16 * 1024 * 1024)?;
        let mut written = 0usize;
        let mut state = RawState::native();
        let mut report = RawReport::new();
        // SAFETY: source exclusive through Engine, stable borrowed identity/options,
        // disjoint initialized bounded outputs, no caller callbacks.
        let code = unsafe {
            ghostty_shellfox_snapshot_budget_encode_buf(
                raw,
                &options,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut written,
                &mut state,
                &mut report,
            )
        };
        let checked_report = SnapshotReport::checked(report, limits, 0, true, code == 0)?;
        if code != 0 {
            if written != 0 || state.size != 0 || state.abi_version != 0 {
                return Err(Error::InvalidOutput("encoder failure published State/body"));
            }
            check(code)?;
        }
        if written == 0 || written > bytes.len() {
            return Err(Error::InvalidOutput("encoded length"));
        }
        let state = ProfileState::checked(state)?;
        bytes.truncate(written);
        let metadata = SnapshotMetadata {
            engine_identity: self.identity,
            wire_profile: 1,
            library_version: 0x00010001,
            snapshot_version: 1,
            state: Some(state),
            configuration: config,
        };
        Ok((NativeSnapshot { bytes, metadata }, checked_report))
    }
    /// No READY terminal escapes. The owned staging handle is freed on every
    /// validation/adoption failure and published as the existing Engine only
    /// after report, State, grid bounds and ownership transfer are checked.
    pub fn decode_native_snapshot(
        &self,
        snapshot: &NativeSnapshot,
        limits: SnapshotLimits,
    ) -> Result<(Engine, SnapshotReport)> {
        limits.validate()?;
        let expected = snapshot.metadata.validate(*self)?;
        self.decode_snapshot_body(snapshot, expected, limits)
    }
    /// Both typed entry points validate their admission policy first. This
    /// shared body path never rewrites metadata or bypasses the native decoder.
    fn decode_snapshot_body(
        &self,
        snapshot: &NativeSnapshot,
        expected: ProfileState,
        limits: SnapshotLimits,
    ) -> Result<(Engine, SnapshotReport)> {
        if snapshot.bytes.len() > limits.max_encoded_bytes {
            return Err(Error::InvalidInput("snapshot encoded bound"));
        }
        let options = DecodeOptions::build(self, limits, snapshot.metadata.configuration);
        let mut raw = std::ptr::null_mut();
        let mut report = RawReport::new();
        // SAFETY: immutable bounded input and options remain live for this
        // synchronous FINISH/EOF transaction; output starts NULL.
        let code = unsafe {
            ghostty_shellfox_snapshot_budget_decode_buf(
                snapshot.bytes.as_ptr(),
                snapshot.bytes.len(),
                &options,
                &mut raw,
                &mut report,
            )
        };
        if code != 0 {
            if !raw.is_null() {
                // SAFETY: quarantine an unexpectedly returned caller-owned handle;
                // never publish it on an error.
                unsafe { ghostty_terminal_free(raw) };
                return Err(Error::InvalidOutput(
                    "decoder failed with non-null terminal",
                ));
            }
            SnapshotReport::checked(report, limits, snapshot.bytes.len(), false, false)?;
            check(code)?;
        }
        let pending = terminal::Staging::result(code, raw)?;
        let checked_report =
            SnapshotReport::checked(report, limits, snapshot.bytes.len(), false, true)?;
        let (cols, rows) = pending.dimensions()?;
        if u32::from(cols) > limits.max_columns
            || u32::from(rows) > limits.max_rows
            || u64::from(cols) * u64::from(rows) > limits.max_cells
        {
            return Err(Error::InvalidOutput("decoded grid bound"));
        }
        if pending.state() != Ok(expected) {
            return Err(Error::InvalidOutput("post-replay profile State"));
        }
        Ok((pending.adopt()?, checked_report))
    }
}
