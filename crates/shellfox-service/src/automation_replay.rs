use crate::{Service, terminals::OwnedTerminal};
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
            || !normal_raw_launch(&terminal.runtime.launch_privilege_facts())
        {
            return None;
        }
        self.completed_automation_exit_code(terminal)
    }
}

fn normal_raw_launch(facts: &LaunchPrivilegeFacts) -> bool {
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
        (Purpose::RawProgram, Basis::OriginalPinnedPosixCredentials) => {
            let owner = rustix::process::getuid();
            owner == rustix::process::geteuid()
                && facts.token.is_none()
                && facts.posix.is_some_and(|p| p.is_non_elevating_account(owner.as_raw()))
        }
        _ => false,
    }
}
