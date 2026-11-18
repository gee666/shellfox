//! Expose only owner-validated presentation identity, never a CoreTab/selection guess.
use super::*;
use crate::Service;
use shellfox_runtime::{Identity, Phase};
impl NativeSshOwner {
    pub(super) fn current_presentation_runtime(
        &self,
        service: &Service,
        controller: u64,
        connection: Uuid,
    ) -> Option<Identity> {
        if !self.grants.contains(&controller) || service.ownership_lost.is_cancelled() {
            return None;
        }
        let state = self.installed.as_ref()?;
        let c = state.manager.connections.get(&connection)?;
        if c.cancel.is_cancelled()
            || !c.reserved
            || (c.controller != controller
                && !(c.authenticated && matches!(c.status.state, State::Ready)))
        {
            return None;
        }
        let expected = c.runtime?;
        if state.runtime.targets.get(&c.status.tab) != Some(&c.target)
            || state.runtime.started.get(&c.status.tab) != Some(&expected)
        {
            return None;
        }
        let actual = &service.terminals.get(&c.status.tab)?.runtime;
        (actual.identity() == expected && actual.phase() != Phase::Closed).then_some(expected)
    }
    pub(super) fn project_status(
        &self,
        service: &Service,
        controller: u64,
        status: &mut ConnectionStatus,
    ) {
        status.runtime = self.current_presentation_runtime(service, controller, status.connection);
    }
    pub(super) fn project_response(
        &self,
        service: &Service,
        controller: u64,
        response: &mut Response,
    ) {
        if let Response::Status(rows) = response {
            for status in rows {
                self.project_status(service, controller, status);
            }
        }
    }
}
