use crate::{terminals::OwnedTerminal, Service};
use shellfox_runtime::{
    LaunchEvidenceBasis as Basis, LaunchEvidenceStage, LaunchFactoryPurpose as Purpose,
    LaunchPrivilege, LaunchPrivilegeFacts, SourceKind,
};

pub(crate) fn is_automation_tab(tab: &shellfox_core::Tab) -> bool {
    tab.profile_id.as_str() == crate::owner_automation::AUTOMATION_PROFILE_ID
}

impl Service {
    pub(crate) fn remote_automation_exit_code(&self, terminal: &OwnedTerminal) -> Option<i64> {
        if self.stopping
            || terminal.runtime.source_kind() != SourceKind::ExternalBytes
            || !matches!(terminal.startup, Some(Ok(())))
            || !normal_raw_launch(&terminal.runtime.launch_privilege_facts(), host_account())
        {
            return None;
        }
        self.completed_automation_exit_code(terminal)
    }
}

#[cfg(unix)]
fn host_account() -> Option<u32> {
    crate::remote::host_account()
}
#[cfg(windows)]
fn host_account() -> Option<u32> {
    None
}

/// A run launched as exactly the host account, root included, with nothing
/// that could gain more. `host` is None when the host's own IDs differ.
#[cfg_attr(windows, allow(unused_variables))]
fn normal_raw_launch(facts: &LaunchPrivilegeFacts, host: Option<u32>) -> bool {
    if facts.privilege != LaunchPrivilege::Normal
        || facts.stage != LaunchEvidenceStage::BeforeRawGoConfirmedReady
        || facts.unknown.is_some()
    {
        return false;
    }
    match (facts.purpose, facts.basis) {
        (Purpose::StandardRawCreateProcess, Basis::ReviewedSupervisorPrimaryTokenInheritance) => {
            cfg!(windows) && facts.token.is_some() && facts.posix.is_none()
        }
        #[cfg(target_os = "linux")]
        (Purpose::RawProgram, Basis::OriginalPinnedPosixCredentials) => host.is_some_and(|owner| {
            facts.token.is_none()
                && facts
                    .posix
                    .is_some_and(|p| p.is_non_elevating_account(owner))
        }),
        // A same-account controller whose target has no setuid/setgid bit.
        #[cfg(target_os = "macos")]
        (Purpose::RawProgram, Basis::OriginalDarwinCredentials { uid }) => {
            host == Some(uid) && facts.token.is_none() && facts.posix.is_none()
        }
        _ => false,
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    fn raw(uid: u32) -> LaunchPrivilegeFacts {
        #[cfg(target_os = "linux")]
        let (basis, posix) = (
            Basis::OriginalPinnedPosixCredentials,
            Some(shellfox_runtime::PosixLaunchFacts {
                uid,
                euid: uid,
                saved_uid: uid,
                fs_uid: uid,
                // Root keeps its account's capabilities under no_new_privs.
                effective_caps: if uid == 0 { u64::MAX } else { 0 },
                permitted_caps: if uid == 0 { u64::MAX } else { 0 },
                inheritable_caps: 0,
                ambient_caps: 0,
                no_new_privs: true,
            }),
        );
        #[cfg(target_os = "macos")]
        let (basis, posix) = (Basis::OriginalDarwinCredentials { uid }, None);
        LaunchPrivilegeFacts {
            privilege: LaunchPrivilege::Normal,
            purpose: Purpose::RawProgram,
            basis,
            stage: LaunchEvidenceStage::BeforeRawGoConfirmedReady,
            token: None,
            posix,
            unknown: None,
        }
    }
    #[test]
    fn a_root_hosts_own_root_runs_replay_remotely() {
        assert!(normal_raw_launch(&raw(0), Some(0)));
        assert!(normal_raw_launch(&raw(1000), Some(1000)));
    }
    #[test]
    fn runs_as_another_account_or_on_a_mixed_id_host_never_replay() {
        assert!(!normal_raw_launch(&raw(1000), Some(0)));
        assert!(!normal_raw_launch(&raw(0), Some(1000)));
        assert!(!normal_raw_launch(&raw(0), None));
        let mut unknown = raw(0);
        unknown.privilege = LaunchPrivilege::Unknown;
        assert!(!normal_raw_launch(&unknown, Some(0)));
    }
}
