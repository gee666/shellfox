use super::RootState;
use shellfox_runtime::Identity;
use std::collections::HashSet;

pub const MAX_OWNED_PROCESSES: usize = 20_000;
pub(super) const MAX_FIELD: usize = 64 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessPlatform {
    Windows,
    Unix,
}
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ProcessIdentity {
    pub domain: String,
    pub pid: u32,
    /// Exact OS birth/boot evidence, never a rounded timestamp.
    pub birth: String,
}
/// Sensitive transient fields. Deliberately neither Debug nor Serialize.
pub struct OwnedProcess {
    pub identity: ProcessIdentity,
    pub is_root: bool,
    pub accessible: bool,
    pub executable: Option<String>,
    pub process_name: Option<String>,
    pub argv: Option<Vec<String>>,
}
pub struct OwnedProcessSnapshot {
    pub identity: Identity,
    pub platform: ProcessPlatform,
    pub root: RootState,
    pub complete: bool,
    pub processes: Vec<OwnedProcess>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionFailure {
    Pending,
    Unavailable,
    Malformed,
    Bounds,
    OwnershipMismatch,
}
impl std::fmt::Display for InspectionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pending => "Owned process inspection is pending.",
            Self::Unavailable => "Owned process inspection is unavailable.",
            Self::Malformed => "Owned process evidence is malformed.",
            Self::Bounds => "Owned process inspection exceeded its bound.",
            Self::OwnershipMismatch => "Owned process evidence belongs to another generation.",
        })
    }
}
impl std::error::Error for InspectionFailure {}
/// Implement on the owner thread using its retained Job/pidfd/supervisor handles.
/// Every row must be an exact authenticated member of this terminal's ownership
/// boundary. This API grants no authority to discover/kill arbitrary numeric PIDs.
/// Implementations must bound collection itself, not merely truncate afterwards.
pub trait ProcessInspector {
    fn inspect(&mut self, identity: Identity) -> Result<OwnedProcessSnapshot, InspectionFailure>;
}
pub(super) fn validate(snapshot: &OwnedProcessSnapshot) -> Result<(), InspectionFailure> {
    if snapshot.processes.len() > MAX_OWNED_PROCESSES {
        return Err(InspectionFailure::Bounds);
    }
    let mut total = 0usize;
    let domain = snapshot
        .processes
        .first()
        .map(|process| process.identity.domain.as_str());
    let mut seen = HashSet::new();
    let mut roots = 0;
    for process in &snapshot.processes {
        if process.identity.pid == 0
            || !seen.insert((&process.identity.domain, process.identity.pid))
        {
            return Err(InspectionFailure::Malformed);
        }
        roots += usize::from(process.is_root);
        if roots > 1 {
            return Err(InspectionFailure::Malformed);
        }
        for field in [&process.identity.domain, &process.identity.birth] {
            if field.is_empty() {
                return Err(InspectionFailure::Malformed);
            }
            if field.len() > 256 {
                return Err(InspectionFailure::Bounds);
            }
        }
        if domain != Some(process.identity.domain.as_str()) {
            return Err(InspectionFailure::OwnershipMismatch);
        }
        let fields = [&process.identity.domain, &process.identity.birth]
            .into_iter()
            .chain(process.executable.iter())
            .chain(process.process_name.iter())
            .chain(process.argv.iter().flatten());
        if process.argv.as_ref().is_some_and(|argv| argv.len() > 4096) {
            return Err(InspectionFailure::Bounds);
        }
        for field in fields {
            if field.len() > MAX_FIELD {
                return Err(InspectionFailure::Bounds);
            }
            if field.contains('\0') {
                return Err(InspectionFailure::Malformed);
            }
            total = total
                .checked_add(field.len())
                .ok_or(InspectionFailure::Bounds)?;
            if total > MAX_SNAPSHOT_BYTES {
                return Err(InspectionFailure::Bounds);
            }
        }
    }
    Ok(())
}
