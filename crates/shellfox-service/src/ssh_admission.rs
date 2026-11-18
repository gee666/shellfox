//! SSH provenance gate only. Existing terminal permissions/leases still own Claim.
use super::*;
use crate::Service;
use anyhow::{ensure, Context};
use shellfox_core::TabId;
use shellfox_runtime::Identity;
impl NativeSshOwner {
    pub fn control_admission(&self, controller: u64, tab: TabId, identity: Identity) -> Result<()> {
        ensure!(
            controller > 0 && identity.tab_id == tab,
            "Invalid terminal control identity."
        );
        let Some(state) = &self.installed else {
            return Ok(());
        };
        let ledger = state.runtime.targets.get(&tab).copied();
        let source = state.manager.source_target(tab);
        if ledger.is_none() && source.is_none() {
            return Ok(());
        }
        ensure!(
            ledger.is_some() && ledger == source,
            "SSH source reservation is inconsistent."
        );
        let target = ledger.context("SSH source reservation missing")?;
        ensure!(
            state.runtime.started.get(&tab) == Some(&identity),
            "SSH runtime generation is stale or not started."
        );
        let connection = state
            .manager
            .connections
            .get(&target.connection())
            .context("SSH connection receipt missing")?;
        ensure!(
            connection.target == target
                && connection.runtime == Some(identity)
                && connection.reserved,
            "SSH connection does not own this exact runtime."
        );
        ensure!(
            connection.authenticated
                && matches!(connection.status.state, State::Ready)
                && !connection.cancel.is_cancelled(),
            "SSH authentication is not Ready; terminal input control cannot be claimed."
        );
        // No native-app grant check or adoption here. Another permitted terminal
        // viewer may Claim a Ready source without revoking/mutating SSH authority.
        Ok(())
    }
}
impl Service {
    pub(crate) fn check_ssh_control_admission(
        &self,
        controller: u64,
        tab: TabId,
        identity: Identity,
    ) -> Result<()> {
        let actual = self
            .terminals
            .get(&tab)
            .context("Terminal control target is not running")?
            .runtime
            .identity();
        ensure!(actual == identity, "Terminal control generation is stale.");
        self.ssh.control_admission(controller, tab, identity)
    }
}
