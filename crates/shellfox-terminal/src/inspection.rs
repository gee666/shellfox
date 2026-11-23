//! Transient read-only evidence from this PTY's retained ownership boundary.
//! No process text in this module implements Debug or Serialize.
use std::ffi::OsString;

pub const MAX_INSPECTION_PROCESSES: usize = 20_000;
pub const MAX_INSPECTION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_INSPECTION_FIELD_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RawProcessIdentity {
    pub pid: u32,
    pub birth: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InspectionPlatform {
    Windows,
    Unix,
}
pub struct RawProcessEvidence {
    pub identity: RawProcessIdentity,
    pub is_root: bool,
    pub accessible: bool,
    pub executable: Option<OsString>,
    pub process_name: Option<OsString>,
    pub argv: Option<Vec<OsString>>,
    /// For `session stats`: parent PID, resident memory, and start time in ms
    /// since the Unix epoch. None where the OS didn't report them.
    pub parent_pid: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub started_ms: Option<u64>,
}
pub struct RawOwnedEvidence {
    pub platform: InspectionPlatform,
    /// The exact launched shell/command, never the waiting supervisor or ConPTY.
    pub root: RawProcessIdentity,
    pub root_alive: bool,
    pub complete: bool,
    pub processes: Vec<RawProcessEvidence>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectionFailure {
    Unsupported,
    Pending,
    Unavailable,
    OwnershipMismatch,
    Bounds,
    Malformed,
}
impl std::fmt::Display for InspectionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "Owned process inspection is unsupported on this platform.",
            Self::Pending => "Owned process inspection is pending.",
            Self::Unavailable => "Owned process inspection is unavailable.",
            Self::OwnershipMismatch => "Owned process identity does not match its retained proof.",
            Self::Bounds => "Owned process inspection exceeded its bound.",
            Self::Malformed => "Owned process inspection data is malformed.",
        })
    }
}
impl std::error::Error for InspectionFailure {}
#[cfg(windows)]
#[path = "inspection_windows.rs"]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{birth, open_query, process_identity, verify_member};
#[cfg(windows)]
#[derive(Clone)]
pub struct InspectionCapability(std::sync::Arc<windows::OwnedInspection>);
#[cfg(target_os = "linux")]
#[derive(Clone)]
pub struct InspectionCapability(pub(crate) std::sync::Arc<crate::unix::linux::inspection::OwnedInspection>);
#[cfg(target_os = "macos")]
#[derive(Clone)]
pub struct InspectionCapability(pub(crate) std::sync::Arc<crate::unix::darwin_process::Inspection>);
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[derive(Clone)]
pub struct InspectionCapability {
    _private: (),
}
impl InspectionCapability {
    #[cfg(windows)]
    pub(crate) fn launch_evidence(
        &self,
        binding: crate::NativeLaunchBinding,
    ) -> crate::NativeLaunchEvidence {
        self.0.launch_evidence(binding)
    }
    /// Blocking read-only collection. Run on a bounded inspection worker, never
    /// on the terminal parser/PTY polling or service typing path.
    pub fn capture(&self) -> Result<RawOwnedEvidence, InspectionFailure> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            self.0.capture()
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Err(InspectionFailure::Unsupported)
        }
    }
    #[cfg(windows)]
    pub(crate) fn from_authenticated_child(
        job: windows_sys::Win32::Foundation::HANDLE,
        supervisor: RawProcessIdentity,
        child: RawProcessIdentity,
    ) -> Result<Self, InspectionFailure> {
        windows::OwnedInspection::new(job, supervisor, child)
            .map(|owned| Self(std::sync::Arc::new(owned)))
    }
}
#[cfg(not(target_os = "linux"))]
impl crate::PtySession {
    /// Clones read-only handles already authenticated before READY. No PID lookup,
    /// command-line read, worker wait, or process-control authority is exposed.
    pub fn inspection_capability(&self) -> Result<InspectionCapability, InspectionFailure> {
        #[cfg(windows)]
        {
            self.inner
                .as_ref()
                .and_then(|inner| inner.inspection.clone())
                .ok_or(InspectionFailure::Unavailable)
        }
        #[cfg(target_os = "macos")]
        {
            self.darwin_inspection()
                .map(|inspection| InspectionCapability(std::sync::Arc::new(inspection)))
                .ok_or(InspectionFailure::Unavailable)
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Err(InspectionFailure::Unsupported)
        }
    }
}
