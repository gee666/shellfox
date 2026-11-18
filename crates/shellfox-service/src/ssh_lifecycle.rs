use super::*;
impl Connection {
    pub(super) fn stop(&mut self) {
        self.cancel.cancel();
        self.auth_cancel.cancel();
        self.pending = None;
        self.view_attached = false;
        // Repeated cancellation/cleanup must not erase the original failure
        // before a native client can observe it, or reopen an already Closed state.
        if !matches!(self.status.state, State::Failed { .. } | State::Closed) {
            self.status.state = State::Closing;
        }
    }
    pub(super) fn detach(&mut self) {
        self.view_attached = false;
        self.pending = None;
        self.auth_cancel.cancel();
        if self.authenticated && !self.cancel.is_cancelled() && self.runtime.is_some() {
            // The SSH session is now host-owned, independent of a presentation/input lease.
            self.status.state = State::Ready;
        } else if !matches!(self.status.state, State::Closed | State::Failed { .. }) {
            self.stop();
        }
    }
}
impl SshManager {
    pub(super) fn runtime_closed_diagnostic(
        &mut self,
        identity: shellfox_runtime::Identity,
        diagnostic: Option<&str>,
    ) {
        let pending: Vec<_> = self
            .connections
            .iter()
            .filter_map(|(id, connection)| {
                (connection.runtime == Some(identity)
                    && !connection.authenticated
                    && !connection.cancel.is_cancelled())
                .then_some(*id)
            })
            .collect();
        self.runtime_closed(identity);
        if let Some(diagnostic) = diagnostic {
            for id in pending {
                let Some(connection) = self.connections.get_mut(&id) else {
                    continue;
                };
                if let State::Failed { message } = &mut connection.status.state {
                    message.push_str(" Public protocol observation: ");
                    message.push_str(diagnostic);
                }
            }
        }
    }
    /// Ordinary view detach/takeover never closes an authenticated SSH process.
    /// Pending auth/prompts are cancelled, with cleanup still held by this manager.
    pub fn detach_controller(&mut self, controller: u64) {
        for c in self
            .connections
            .values_mut()
            .filter(|c| c.controller == controller)
        {
            c.detach();
        }
    }
    /// Explicit security/capability reduction, NOT ordinary input/view detach.
    pub fn revoke_native_capability(&mut self, controller: u64) {
        for c in self
            .connections
            .values_mut()
            .filter(|c| c.controller == controller)
        {
            c.stop();
        }
    }
    /// Read-only source lookup for the SAME owner used by local attach/start paths.
    /// Keep the reservation until every pending worker/runtime/credential owner is closed.
    pub fn source_target(&self, tab: shellfox_core::TabId) -> Option<SourceConnectTarget> {
        self.connections
            .values()
            .find(|c| c.status.tab == tab && c.reserved)
            .map(|c| c.target)
    }
    pub fn ensure_local_start_allowed(&self, tab: shellfox_core::TabId) -> Result<()> {
        anyhow::ensure!(
            self.source_target(tab).is_none(),
            "This tab is reserved for SSH; a local shell must not auto-start."
        );
        Ok(())
    }
    pub fn adopt_ready(
        &mut self,
        controller: u64,
        connection: Uuid,
        validator: &dyn NativeControllerValidator,
        starter: &dyn OwnedRuntimeStarter,
    ) -> Result<ConnectionStatus> {
        let c = self
            .connections
            .get(&connection)
            .ok_or_else(|| anyhow::anyhow!("SSH connection is unavailable."))?;
        self.check(controller, Some(c.status.tab), validator)?;
        anyhow::ensure!(
            c.authenticated
                && matches!(c.status.state, State::Ready)
                && !c.cancel.is_cancelled()
                && c.pending.is_none()
                && c.reserved,
            "Only an authenticated Ready SSH session can be adopted."
        );
        let identity = c
            .runtime
            .ok_or_else(|| anyhow::anyhow!("SSH runtime is unavailable."))?;
        anyhow::ensure!(
            starter.runtime_is_current(identity)?,
            "SSH runtime generation is stale or closed."
        );
        self.check(controller, Some(c.status.tab), validator)?;
        let c = self
            .connections
            .get_mut(&connection)
            .ok_or_else(|| anyhow::anyhow!("SSH connection disappeared."))?;
        c.controller = controller;
        c.view_attached = true;
        // No Claim, attach, resize, auth restart or Runtime mutation occurs here.
        Ok(c.status.clone())
    }
    pub(super) fn visible_statuses(
        &self,
        controller: u64,
        id: Option<Uuid>,
        validator: &dyn NativeControllerValidator,
    ) -> Vec<ConnectionStatus> {
        let mut rows: Vec<_> = self
            .connections
            .values()
            .filter(|c| {
                id.is_none_or(|id| id == c.status.connection)
                    && (c.controller == controller
                        || (c.authenticated && matches!(c.status.state, State::Ready)))
                    && validator.validate_target(controller, c.status.tab).is_ok()
            })
            .map(|c| c.status.clone())
            .collect();
        rows.sort_by_key(|r| r.connection);
        rows
    }
}
