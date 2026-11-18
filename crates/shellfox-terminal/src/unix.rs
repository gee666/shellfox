//! Unix lifecycle contracts and native ownership adapters.
//!
//! Linux production sessions require cgroup v2 delegation and an original child
//! controller. Darwin remains gated pending its native ownership prerequisite.
#[cfg(target_os = "linux")]
mod cgroup;
#[cfg(target_os = "linux")]
pub(crate) mod environment;
mod lifecycle;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod private;
#[cfg(target_os = "linux")]
pub(crate) mod production_supervisor;
mod protocol;
#[cfg(target_os = "linux")]
pub(crate) mod raw;
#[cfg(target_os = "linux")]
pub(crate) mod session;
#[cfg(target_os = "linux")]
mod startup;
#[cfg(target_os = "linux")]
mod state;
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
mod tree;

pub use lifecycle::{Lifecycle, Ownership, Phase};
pub use protocol::{Capability, ControlCommand, ControlIdentity, SupervisorStatus};

// Native fixtures exercise the real PTY adapter without changing the production
// unsupported-platform gate. A cross-check alone cannot justify that change.
#[cfg(all(
    feature = "test-support",
    any(target_os = "linux", target_os = "macos")
))]
pub mod native_session;

#[cfg(target_os = "macos")]
pub mod darwin;
#[cfg(target_os = "linux")]
pub mod linux;

// The Linux stat format is a pure contract and can be tested on every host.
#[cfg(all(test, not(target_os = "linux")))]
#[path = "unix/linux/stat.rs"]
mod linux_stat;

#[cfg(test)]
mod tests;
