//! PTY sessions with owned Windows Job and Linux cgroup-v2 boundaries.
mod config;
pub mod elevated;
mod environment;
#[cfg(windows)]
mod environment_paths;
mod error;
#[cfg(any(windows, target_os = "linux"))]
mod input;
pub mod inspection;
#[cfg(windows)]
mod inspection_gate;
mod isolation;
pub mod launch_privilege;
pub use launch_privilege::{NativeLaunchBinding, NativeLaunchEvidence};
mod output;
mod profile;
pub mod program_env;
#[cfg(windows)]
mod program_env_pipe;
pub use program_env::{AskpassBroker, TrustedProgramEnv, TrustedProgramLease};
pub mod raw_process;
#[cfg(any(windows, target_os = "linux", test))]
mod reader;
#[cfg(windows)]
mod runtime;
#[cfg(feature = "test-support")]
mod test_support;
pub mod unix;
pub mod wsl;
#[cfg(feature = "test-support")]
pub use test_support::StartupFault;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod supervisor;
#[cfg(not(any(windows, target_os = "linux")))]
mod unsupported;
#[cfg(windows)]
mod windows;

pub use config::{PtyCommand, PtyConfig, UnixPrivilegeMode};
pub use environment::EnvironmentPolicy;
pub use error::SpawnError;
pub use isolation::{validate_isolated_path, IsolationPathKind};
pub use output::{PtyOutput, OUTPUT_CHUNK_BYTES, OUTPUT_QUEUE_CHUNKS};
pub use portable_pty::ExitStatus;
pub use profile::{
    default_shell_profile, discover_shell_profiles, shell_profile, ShellProfile, ShellProfileId,
};
#[cfg(windows)]
pub use session::PtySession;
#[cfg(target_os = "linux")]
pub use unix::session::PtySession;
#[cfg(not(any(windows, target_os = "linux")))]
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
