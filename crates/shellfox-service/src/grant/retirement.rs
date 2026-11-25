use crate::grant::custody::ActualGrant;
use crate::*;
use shellfox_runtime::{Command, Event, Identity, Request};
impl Service {
    pub(crate) fn record_ownership_reply(&mut self, identity: Identity, event: &Event) {
        self.grants.record_revocation(identity, event);
        if let Event::ControllerRevoked { lease, .. } = event {
            if let Some(terminal) = self.terminals.get_mut(&identity.tab_id) {
                if terminal.runtime.identity() == identity {
                    terminal.forget_lease(*lease);
                }
            }
        }
        if let Event::Reply { id, result } = event {
            let _ = self.grants.record_reply(identity, *id, result);
        }
    }
    pub(crate) fn pump_grant_retirements(&mut self) -> Result<()> {
        let retiring: Vec<_> = self
            .terminals
            .values()
            .filter_map(|terminal| {
                terminal
                    .retiring_lease()
                    .map(|lease| (terminal.runtime.identity(), lease))
            })
            .collect();
        for (identity, lease) in retiring {
            let id = self.request_id()?;
            if self
                .grants
                .retire_owned(identity, id, ActualGrant::Controller(lease))
                .is_ok()
            {
                if let Some(terminal) = self.terminals.get_mut(&identity.tab_id) {
                    terminal.forget_lease(lease);
                }
            }
        }
        for (ticket, actual, cleanup) in self.grants.retirements() {
            if self
                .terminals
                .get(&ticket.identity().tab_id)
                .is_none_or(|terminal| terminal.runtime.identity() != ticket.identity())
            {
                continue;
            }
            let id = match cleanup {
                Some((_, true)) => continue,
                Some((id, false)) => id,
                None => {
                    let id = self.request_id()?;
                    self.grants.prepare_retirement(ticket, id).map_err(|_| {
                        anyhow::anyhow!(
                            "Grant retirement reservation failed; the host keeps the grant."
                        )
                    })?;
                    id
                }
            };
            let command = match actual {
                ActualGrant::Controller(lease) => Command::ReleaseControl { lease },
                ActualGrant::Observer(observer) => Command::Unobserve { observer },
            };
            let terminal = self
                .terminals
                .get(&ticket.identity().tab_id)
                .context("Grant source owner disappeared.")?;
            match terminal.runtime.try_send(Request { id, command }) {
                Ok(()) => self.grants.retirement_sent(ticket, id).map_err(|_| {
                    anyhow::anyhow!("Grant cleanup became uncertain; the host keeps the grant.")
                })?,
                Err(shellfox_runtime::SubmitError::Full(_)) => {}
                Err(_) => {} // No inference from a closed/invalid queue; wait for exact source destruction.
            }
        }
        self.grants.reap_closed();
        Ok(())
    }
    /// Sends the release of `controller`'s detached lease on `tab` and waits
    /// briefly until the runtime applied it. A view that detached can then
    /// claim the terminal again at once, as in 0.9.
    pub(crate) fn settle_controller_release(&mut self, tab: TabId, controller: u64) {
        let Some(identity) = self.terminals.get(&tab).map(|t| t.runtime.identity()) else {
            return;
        };
        let kind = crate::grant::custody::GrantKind::Controller(controller);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let _ = self.wait_until(
            deadline,
            |service| {
                service.pump_grant_retirements().map_err(|_| ())?;
                service.drain(tab).map_err(|_| ())?;
                Ok((!service.grants.releasing(identity, kind)).then_some(()))
            },
            || (),
        );
    }
    /// Only after the original source owner was authoritatively removed by its
    /// existing cleanup path. Source Finish/Phase never calls this callback.
    pub(crate) fn ownership_source_destroyed(&mut self, identity: Identity) {
        self.grants.destroy_source(identity);
    }
}
