//! PTY sessions with Windows Jobs, Linux cgroups, and macOS session cleanup.
mod config;
pub mod elevated;
mod environment;
#[cfg(windows)]
mod environment_paths;
#[cfg(any(windows, target_os = "linux", target_os = "macos", test))]
mod environment_user;
mod error;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod input;
pub mod inspection;
#[cfg(windows)]
mod inspection_gate;
mod isolation;
pub mod launch_privilege;
pub use launch_privilege::{NativeLaunchBinding, NativeLaunchEvidence};
mod output;
mod profile;
pub mod raw_process;
#[cfg(any(windows, target_os = "linux", target_os = "macos", test))]
mod reader;
#[cfg(windows)]
mod runtime;
#[cfg(feature = "test-support")]
mod test_support;
pub mod unix;
mod wake;
pub mod wsl;
#[cfg(feature = "test-support")]
pub use test_support::StartupFault;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod supervisor;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod unsupported;
#[cfg(windows)]
mod windows;

pub use config::{PtyCommand, PtyConfig, UnixPrivilegeMode};
pub use environment::{EnvironmentPolicy, TerminalEnvironment};
pub use error::SpawnError;
pub(crate) use isolation::{validate_isolated_path, IsolationPathKind};
pub use output::{PtyOutput, OUTPUT_CHUNK_BYTES, OUTPUT_QUEUE_CHUNKS};
pub use portable_pty::ExitStatus;
pub use wake::{ExitSignal, Wake};
pub use profile::{
    default_shell_profile, discover_shell_profiles, shell_profile, ShellProfile, ShellProfileId,
};
#[cfg(windows)]
pub use session::PtySession;
#[cfg(target_os = "macos")]
pub use unix::darwin_session::PtySession;
#[cfg(target_os = "linux")]
pub use unix::session::PtySession;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub use unsupported::PtySession;

/// Call before initializing any GUI or starting threads.
/// Returns the supervised command's exit code only in supervisor mode.
pub fn supervisor_entry() -> anyhow::Result<Option<u32>> {
    #[cfg(windows)]
    {
        supervisor::entry()
    }
    #[cfg(target_os = "linux")]
    {
        unix::production_supervisor::entry()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(None)
    }
}
