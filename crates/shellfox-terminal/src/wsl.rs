//! WSL guest ownership is separate from the Windows transport Job.
//! Nested inherited markers are status only and never accepted by this API.
// Native launch constructors are intentionally unavailable off Windows.
#[cfg_attr(not(any(windows, feature = "test-support")), allow(dead_code))]
#[path = "wsl/config.rs"]
mod config;
#[path = "wsl/discovery.rs"]
mod discovery;
#[path = "wsl/discovery_process.rs"]
mod discovery_process;
#[path = "wsl/guest_processes.rs"]
mod guest_processes;
#[path = "wsl/protocol.rs"]
mod protocol;
#[cfg_attr(not(any(windows, feature = "test-support")), allow(dead_code))]
#[path = "wsl/session.rs"]
mod session;
#[path = "wsl/tunnel.rs"]
mod tunnel;
#[path = "wsl/upload.rs"]
mod upload;
#[cfg(windows)]
#[path = "wsl/windows.rs"]
mod windows;

pub use config::{
    GuestCommand, GuestSettings, NativeWslCapability, WslConfig, WslConsent, WslSecret,
};
pub use discovery::{decode_distro_list, OsWsl, WslProfileId};
pub use guest_processes::{parse_guest_processes, GuestProcess, GuestProcesses};
pub use protocol::{GuardianAnchor, GuestIdentity, GuestStatus, RecoveryEvidence, WslError};
pub use session::{WslGuestGuardian, WslSession, WslSpawnError};
pub use upload::WslUploadMapping;

pub(crate) const BOOTSTRAP: &str = include_str!("../../../native/wsl/bootstrap.py");
pub(crate) const SOURCES: [(&str, &str); 4] = [
    (
        "shellfox_wsl_wire",
        include_str!("../../../native/wsl/wire.py"),
    ),
    (
        "shellfox_wsl_tree",
        include_str!("../../../native/wsl/tree.py"),
    ),
    (
        "shellfox_wsl_child",
        include_str!("../../../native/wsl/child.py"),
    ),
    (
        "shellfox_wsl_guardian",
        include_str!("../../../native/wsl/guardian.py"),
    ),
];
