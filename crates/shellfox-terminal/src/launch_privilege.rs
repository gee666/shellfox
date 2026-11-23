//! Sealed descriptive launch facts. Never a share grant or process-control right.
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPrivilege {
    Normal,
    Elevated,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchFactoryPurpose {
    Unbound,
    OwnedPtyProgram,
    StandardRawCreateProcess,
    RawProgram,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchEvidenceBasis {
    Unavailable,
    OriginalPinnedChildToken,
    ReviewedSupervisorPrimaryTokenInheritance,
    OriginalPinnedPosixCredentials,
    /// Same real, effective and saved UID at macOS PTY launch. No claim about later execs.
    OriginalDarwinCredentials {
        uid: u32,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchEvidenceStage {
    NotCaptured,
    BeforePublicPtyReady,
    BeforeRawGoConfirmedReady,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchUnknownReason {
    NotCaptured,
    Unsupported,
    OwnershipUncertain,
    TokenQueryUnavailable,
    InconsistentToken,
    UnrestrictedPosixLaunch,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsElevationType {
    Default,
    Full,
    Limited,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsLaunchToken {
    pub elevated: bool,
    pub elevation_type: WindowsElevationType,
    pub integrity_rid: u32,
    pub ui_access: bool,
}
/// Measured from the original pidfd-pinned controller before GO. The Linux
/// factory permits Normal only with no_new_privs inherited by all children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosixLaunchFacts {
    pub uid: u32,
    pub euid: u32,
    pub saved_uid: u32,
    pub fs_uid: u32,
    pub effective_caps: u64,
    pub permitted_caps: u64,
    pub inheritable_caps: u64,
    pub ambient_caps: u64,
    pub no_new_privs: bool,
}
impl PosixLaunchFacts {
    pub fn is_non_elevating_account(&self, uid: u32) -> bool {
        uid != 0
            && [self.uid, self.euid, self.saved_uid, self.fs_uid] == [uid; 4]
            && [
                self.effective_caps,
                self.permitted_caps,
                self.inheritable_caps,
                self.ambient_caps,
            ] == [0; 4]
            && self.no_new_privs
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchPrivilegeFacts {
    pub privilege: LaunchPrivilege,
    pub purpose: LaunchFactoryPurpose,
    pub basis: LaunchEvidenceBasis,
    pub stage: LaunchEvidenceStage,
    pub token: Option<WindowsLaunchToken>,
    pub posix: Option<PosixLaunchFacts>,
    pub unknown: Option<LaunchUnknownReason>,
}
impl Default for LaunchPrivilegeFacts {
    fn default() -> Self {
        Self {
            privilege: LaunchPrivilege::Unknown,
            purpose: LaunchFactoryPurpose::Unbound,
            basis: LaunchEvidenceBasis::Unavailable,
            stage: LaunchEvidenceStage::NotCaptured,
            token: None,
            posix: None,
            unknown: Some(LaunchUnknownReason::NotCaptured),
        }
    }
}
struct Seal;
/// Correlation only, NOT authorization to spawn/share. Never Clone/Serde.
pub struct NativeLaunchBinding(Arc<Seal>);
pub struct LaunchBindingVerifier(Arc<Seal>);
/// A new local binding cannot validate evidence for an existing Runtime. Runtime
/// keeps its verifier private and only gives its one binding to the native factory.
pub fn new_launch_binding() -> (NativeLaunchBinding, LaunchBindingVerifier) {
    let seal = Arc::new(Seal);
    (
        NativeLaunchBinding(Arc::clone(&seal)),
        LaunchBindingVerifier(seal),
    )
}
#[cfg(target_os = "macos")]
pub(crate) fn capture_darwin(binding: NativeLaunchBinding, uid: u32) -> NativeLaunchEvidence {
    NativeLaunchEvidence {
        seal: binding.0,
        facts: LaunchPrivilegeFacts {
            privilege: if uid == 0 {
                LaunchPrivilege::Elevated
            } else {
                LaunchPrivilege::Normal
            },
            purpose: LaunchFactoryPurpose::OwnedPtyProgram,
            basis: LaunchEvidenceBasis::OriginalDarwinCredentials { uid },
            stage: LaunchEvidenceStage::BeforePublicPtyReady,
            token: None,
            posix: None,
            unknown: None,
        },
    }
}
/// The raw controller's own credentials before GO. A setuid/setgid target can
/// still change them at exec, so its launch is not certified as Normal.
#[cfg(target_os = "macos")]
pub(crate) fn capture_darwin_raw(
    binding: NativeLaunchBinding,
    uid: u32,
    setid_target: bool,
) -> NativeLaunchEvidence {
    let mut evidence = capture_darwin(binding, uid);
    evidence.facts.purpose = LaunchFactoryPurpose::RawProgram;
    evidence.facts.stage = LaunchEvidenceStage::BeforeRawGoConfirmedReady;
    if setid_target && uid != 0 {
        evidence.facts.privilege = LaunchPrivilege::Unknown;
        evidence.facts.unknown = Some(LaunchUnknownReason::UnrestrictedPosixLaunch);
    }
    evidence
}

#[cfg(target_os = "linux")]
#[path = "launch_privilege_posix.rs"]
mod posix;
#[cfg(target_os = "linux")]
pub(crate) use posix::{capture as capture_posix, capture_raw as capture_raw_posix};
#[cfg(windows)]
#[path = "launch_privilege_windows.rs"]
mod platform;
/// Constructors are factory-private. No enum/PID/path setter can create this.
pub struct NativeLaunchEvidence {
    seal: Arc<Seal>,
    facts: LaunchPrivilegeFacts,
    #[cfg(windows)]
    _proof: Option<platform::InitialProof>,
}
impl std::fmt::Debug for NativeLaunchEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeLaunchEvidence([sealed initial-launch proof])")
    }
}
impl NativeLaunchEvidence {
    pub fn facts(&self) -> LaunchPrivilegeFacts {
        self.facts
    }
}
impl LaunchBindingVerifier {
    pub fn matches(&self, evidence: &NativeLaunchEvidence) -> bool {
        Arc::ptr_eq(&self.0, &evidence.seal)
    }
    pub fn matches_binding(&self, binding: &NativeLaunchBinding) -> bool {
        Arc::ptr_eq(&self.0, &binding.0)
    }
}
#[cfg(any(windows, test))]
fn classify(token: WindowsLaunchToken) -> (LaunchPrivilege, Option<LaunchUnknownReason>) {
    let inconsistent = token.elevation_type == WindowsElevationType::Limited && token.elevated
        || token.elevation_type == WindowsElevationType::Full && !token.elevated;
    if inconsistent {
        return (
            LaunchPrivilege::Unknown,
            Some(LaunchUnknownReason::InconsistentToken),
        );
    }
    if token.elevated {
        return (LaunchPrivilege::Elevated, None);
    }
    // High/System integrity or UIAccess never becomes a false Normal source.
    if token.integrity_rid >= 0x3000 || token.ui_access {
        return (
            LaunchPrivilege::Unknown,
            Some(LaunchUnknownReason::InconsistentToken),
        );
    }
    (LaunchPrivilege::Normal, None)
}
#[cfg(windows)]
fn capture(
    binding: NativeLaunchBinding,
    process: windows_sys::Win32::Foundation::HANDLE,
    job: windows_sys::Win32::Foundation::HANDLE,
    purpose: LaunchFactoryPurpose,
    basis: LaunchEvidenceBasis,
    stage: LaunchEvidenceStage,
) -> NativeLaunchEvidence {
    let captured = platform::capture(process, job);
    let (token, proof, unknown) = match captured {
        Ok((token, proof)) => (Some(token), Some(proof), None),
        Err(reason) => (None, None, Some(reason)),
    };
    let (privilege, unknown) = token.map_or((LaunchPrivilege::Unknown, unknown), classify);
    NativeLaunchEvidence {
        seal: binding.0,
        facts: LaunchPrivilegeFacts {
            privilege,
            purpose,
            basis: if proof.is_some() {
                basis
            } else {
                LaunchEvidenceBasis::Unavailable
            },
            stage,
            token,
            posix: None,
            unknown,
        },
        _proof: proof,
    }
}
#[cfg(windows)]
pub(crate) fn capture_pinned_pty(
    binding: NativeLaunchBinding,
    process: windows_sys::Win32::Foundation::HANDLE,
    job: windows_sys::Win32::Foundation::HANDLE,
) -> NativeLaunchEvidence {
    capture(
        binding,
        process,
        job,
        LaunchFactoryPurpose::OwnedPtyProgram,
        LaunchEvidenceBasis::OriginalPinnedChildToken,
        LaunchEvidenceStage::BeforePublicPtyReady,
    )
}
/// Only the fixed STANDARD raw supervisor factory may call this before GO. It
/// uses ordinary CreateProcess with its own primary token and no runas, alternate
/// token, ShellExecute or impersonation spawn. This is NOT a generic provider.
#[cfg(windows)]
pub(crate) fn capture_standard_raw_before_go(
    binding: NativeLaunchBinding,
    supervisor: windows_sys::Win32::Foundation::HANDLE,
    job: windows_sys::Win32::Foundation::HANDLE,
) -> PendingRawLaunchEvidence {
    PendingRawLaunchEvidence(capture(
        binding,
        supervisor,
        job,
        LaunchFactoryPurpose::StandardRawCreateProcess,
        LaunchEvidenceBasis::ReviewedSupervisorPrimaryTokenInheritance,
        LaunchEvidenceStage::BeforeRawGoConfirmedReady,
    ))
}
#[cfg(windows)]
pub(crate) struct PendingRawLaunchEvidence(NativeLaunchEvidence);
#[cfg(windows)]
impl PendingRawLaunchEvidence {
    pub(crate) fn confirm_ready(self) -> NativeLaunchEvidence {
        self.0
    }
}
#[cfg(all(windows, feature = "test-support"))]
pub fn fixture_current_token() -> Result<WindowsLaunchToken, LaunchUnknownReason> {
    platform::read_token(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() })
}
#[cfg(test)]
#[path = "launch_privilege_tests.rs"]
mod tests;
