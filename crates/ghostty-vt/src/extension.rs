//! Opt-in, verified Native002 compatibility operations on the existing Terminal.
//! This module is not compiled by default and does not select an app engine.
//! Verification checks declared compatibility, not artifact authenticity.
use crate::{sys, Terminal};
use sys::shellfox_extension as native;
#[path = "extension_ansi.rs"]
mod ansi;
#[path = "extension_profile.rs"]
mod profile;
#[path = "extension_snapshot.rs"]
mod snapshot;
pub use ansi::NativeAnsi;
pub use native::{
    AnsiLimits, EngineIdentity, Error as ExtensionError, ProfileState, RestorationConfig,
    SnapshotLimits, SnapshotMetadata, SnapshotReport, TerminalProfile,
};
pub use snapshot::{CrossTargetNative0RawV1Snapshot, Native0RawV1Compatibility, NativeSnapshot};
pub type ExtensionResult<T> = std::result::Result<T, ExtensionError>;

/// Obtained only by verifying the private opt-in library against the producer's
/// compiled expected profile and exact Native002 ABIs. Not an authorization.
#[derive(Debug, Clone, Copy)]
pub struct VerifiedNativeExtension {
    inner: native::VerifiedNativeExtension,
}
impl VerifiedNativeExtension {
    pub fn verify() -> ExtensionResult<Self> {
        native::VerifiedNativeExtension::verify().map(|inner| Self { inner })
    }
    pub fn verify_expected(identity: EngineIdentity) -> ExtensionResult<Self> {
        native::VerifiedNativeExtension::verify_expected(identity).map(|inner| Self { inner })
    }
    pub fn identity(&self) -> EngineIdentity {
        self.inner.identity()
    }
}
impl Terminal {
    fn from_extension_engine(mut engine: sys::Engine) -> ExtensionResult<Self> {
        let dimensions = engine.recovery_info().map_err(ExtensionError::Native)?;
        Ok(Self {
            engine,
            cols: dimensions.cols,
            rows: dimensions.rows,
        })
    }
}
