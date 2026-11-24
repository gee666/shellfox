//! Unix lifecycle contracts and native ownership adapters.
//!
//! Linux isolates sessions and automation runs with cgroup v2 when the account
//! has a systemd user delegation, otherwise like Darwin: an original session
//! leader and best-effort cleanup within its session ID.
#[cfg(target_os = "linux")]
mod boundary;
#[cfg(target_os = "linux")]
mod cgroup;
#[cfg(target_os = "macos")]
mod darwin_io;
#[cfg(target_os = "macos")]
mod darwin_kqueue;
#[cfg(target_os = "macos")]
pub(crate) mod darwin_process;
#[cfg(target_os = "macos")]
pub(crate) mod darwin_session;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod environment;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod handshake;
mod lifecycle;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod private;
#[cfg(target_os = "linux")]
pub(crate) mod production_supervisor;
mod protocol;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod raw;
#[cfg(target_os = "linux")]
pub(crate) mod session;
#[cfg(target_os = "linux")]
mod startup;
#[cfg(target_os = "linux")]
mod state;
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
mod tree;

#[cfg(all(target_os = "linux", feature = "test-support"))]
pub use boundary::FORCE_SESSION;
#[cfg(target_os = "linux")]
pub use boundary::{containment, Containment};
pub use lifecycle::{Lifecycle, Ownership, Phase};
pub use protocol::{Capability, ControlCommand, ControlIdentity, SupervisorStatus};

// Explicit fixtures for the separate native supervisor adapters.
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
