//! Local-host automation with ordinary byte pipes, never a PTY.
#[path = "raw_process_command.rs"]
pub(crate) mod command;
#[path = "raw_process_io.rs"]
pub(crate) mod io;
#[cfg(windows)]
#[path = "raw_process_windows.rs"]
mod platform;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    pub use crate::unix::raw::{RawProcess, RawSpawnError};
}
#[cfg(windows)]
#[path = "raw_process_supervisor.rs"]
mod supervisor;

pub use command::RawCommand;
pub use io::{
    AutomationStdinClosed, AutomationStdinIoError, AutomationStdinUnavailable, RawOutput,
    RawStream, WriteReceipt, CHUNK_BYTES, INPUT_BYTES, OUTPUT_CREDIT,
};
pub use platform::{RawProcess, RawSpawnError};

/// Run before GUI, logging or normal argument parsing. Supervisor diagnostics must
/// never contaminate the target's stdout/stderr. Failure is reported by no READY.
pub fn supervisor_entry() -> Option<u32> {
    #[cfg(windows)]
    {
        supervisor::entry()
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        crate::unix::raw::supervisor_entry()
    }
}
