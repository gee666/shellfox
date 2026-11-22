//! Retained Go Tailscale boundaries. Transport success is not host approval.
pub use shellfox_protocol::{
    AuthenticatedPeerId, CancellationToken, Endpoint, OutboundTransport, RequestLimits,
    TransportError, TransportFuture,
};
pub use shellfox_protocol::{MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES};

#[cfg(feature = "helper")]
mod config;
#[cfg(feature = "helper")]
mod helper;
#[cfg(feature = "helper")]
mod http;
#[cfg(feature = "helper")]
mod limits;
#[cfg(feature = "helper")]
mod http_status;
#[cfg(feature = "helper")]
pub use http_status::HttpResponse;
#[cfg(feature = "host-auth")]
pub mod incoming;
#[cfg(feature = "host-auth")]
mod secret;
#[cfg(feature = "host-auth")]
pub use secret::Secret;
#[cfg(all(feature = "helper", target_os = "macos"))]
mod acl_policy;
#[cfg(feature = "helper")]
mod native_admission;
#[cfg(feature = "helper")]
mod offline;
#[cfg(feature = "helper")]
mod offline_control;
#[cfg(feature = "helper")]
pub use native_admission::{
    AcquiredOwnershipView, NativeAdmission, NativeOwnershipFence, OriginalCleanupView,
};
#[cfg(feature = "helper")]
mod offline_scope;
#[cfg(feature = "helper")]
mod ownership_root;
#[cfg(feature = "helper")]
mod root_lifetime;
#[cfg(feature = "helper")]
pub use root_lifetime::RootLifetimeGuard;
#[cfg(feature = "helper")]
mod preparation;
#[cfg(all(feature = "helper", unix))]
mod unix_acl;
#[cfg(feature = "helper")]
pub use ownership_root::{BoundOwnershipRoot, OwnershipRootIdentity};
#[cfg(feature = "helper")]
pub use preparation::{
    AcquireTransferFailure, BindRootFailure, NoOwnerProof, OfflinePreparation, OwnershipAttempt,
    OwnershipDecision,
};
#[cfg(all(feature = "helper", unix))]
pub use unix_acl::verify_held_acl;
#[cfg(all(feature = "helper", target_os = "macos"))]
pub use unix_acl::{tighten_held_acl, verify_held_traversal_acl};
#[cfg(feature = "helper")]
mod permissions;
#[cfg(feature = "helper")]
mod private_path;
#[cfg(feature = "helper")]
mod process;
#[cfg(feature = "helper")]
mod protocol;
#[cfg(feature = "helper")]
mod retirement;
#[cfg(feature = "helper")]
pub use config::{HelperConfig, IsolatedStateDirectory, LoopbackOrigin, VerifiedHelper};
#[cfg(feature = "helper")]
pub use helper::{HelperShutdown, HelperTransport};
#[cfg(feature = "helper")]
pub use offline::HostOwnership;
#[cfg(feature = "helper")]
pub use permissions::{PermissionVerifier, PrivatePathKind};
#[cfg(feature = "helper")]
pub use private_path::normalize_private_path;
#[cfg(feature = "helper")]
pub use protocol::{NetworkPeer, NetworkState, NetworkStatus, Probe};

pub const HELPER_PROTOCOL_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperError {
    Configuration,
    InvalidRequest,
    PermissionDenied,
    UnsupportedIdentity,
    OwnershipRequired,
    RootChainRequired,
    StateUnmarked,
    StateMarkerInvalid,
    Executable,
    Spawn,
    Protocol,
    ProtocolVersion { found: u32 },
    Command,
    Deadline,
    Stopped,
    Busy,
}
impl std::fmt::Display for HelperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Configuration => "Invalid helper configuration.",
            Self::InvalidRequest => "Invalid helper path form.",
            Self::PermissionDenied => "Unsafe private permissions.",
            Self::UnsupportedIdentity => "Native full-width root identity is unsupported; no fallback was used.",
            Self::OwnershipRequired => "Existing permissions cannot be rewritten before actual ownership.",
            Self::RootChainRequired => "Original full-chain directory custody is required before native work or binding.",
            Self::StateUnmarked => "State folder has no Shellfox Rust ownership marker; existing node credentials were not adopted.",
            Self::StateMarkerInvalid => "Rust node state marker is invalid, unsupported, or belongs to another fixed path.",
            Self::Executable => "Helper artifact verification failed.",
            Self::Spawn => "Cannot start helper.",
            Self::Protocol => "Invalid helper protocol.",
            Self::ProtocolVersion { found } => return write!(f, "Incompatible Tailscale helper protocol: expected {HELPER_PROTOCOL_VERSION}, found {found}. Rebuild the helper or reinstall Shellfox."),
            Self::Command => "Helper command failed.",
            Self::Deadline => "Helper timed out.",
            Self::Stopped => "Helper stopped.",
            Self::Busy => "Helper command already pending.",
        })
    }
}
impl std::error::Error for HelperError {}
