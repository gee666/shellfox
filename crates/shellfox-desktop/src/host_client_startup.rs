/// Resource ownership proof published by the service bootstrap, not inferred
/// from a failed ready receiver, endpoint-file absence, or a finished thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostEndpointStartupOutcome {
    /// No helper/actor was acquired, or their explicit shutdown and joins finished.
    NoOwnerClean,
    /// Startup is pending or acquired resources still need confirmed cleanup.
    RetainedCleanup,
    /// The common local endpoint and service owner are published and usable.
    Ready,
    /// Actual local readiness with the requested remote feature unavailable.
    ReadyLocalOnly {
        remote_unavailable: shellfox_service::remote::RemoteUnavailableReason,
    },
}
