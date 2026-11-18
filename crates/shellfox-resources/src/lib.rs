//! Host-only native payload verification. A digest is an external trust anchor,
//! not publisher authentication. Keep the verified bundle until exit is confirmed.
#[cfg(any(target_os = "android", target_os = "ios"))]
compile_error!("shellfox-resources is host-only; mobile must not depend on native host helpers");

mod bundle;
mod contracts;
mod digest;
mod loader;
mod managed;
mod manifest;
mod platform;
mod process;
mod tree;

pub use bundle::{VerifiedBundle, VerifiedExecutable};
pub use contracts::capabilities as expected_capabilities;
pub use digest::{compiled_manifest_anchor, ExpectedManifestDigest, TrustedManifestDigest};
pub use loader::NoDynamicCodeLoad;
pub use manifest::{
    BundleFile, BundleIdentity, BundleManifest, Capability, EntryPoint, FileSet, ProtocolContract,
    ProtocolId, ResourceRuntime, RuntimeId, TrustedBundleId, MANIFEST_NAME,
};
pub use process::{reap_unconfirmed_children, VerifiedChild, VerifiedCommand};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native bundle I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid native bundle manifest: {0}")]
    Manifest(&'static str),
    #[error("native bundle manifest digest differs from the external trust anchor")]
    ManifestDigest,
    #[error("native bundle identity differs from the expected build")]
    Identity,
    #[error("native bundle file set or content changed: {0}")]
    Changed(String),
    #[error("native bundle contains an alias, link or unsupported file: {0}")]
    Alias(String),
    #[error("immutable native path execution has not been established on this platform")]
    ImmutableExecutionUnavailable,
    #[error("managed execution requires a manifest-bound trusted closed-loader review; probes do not grant this policy")]
    LoaderPolicyRequired,
}

pub type Result<T> = std::result::Result<T, Error>;
