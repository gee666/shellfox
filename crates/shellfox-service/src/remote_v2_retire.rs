//! Invalidate view custody first; retain bounded idempotent detach work after it.
use crate::Service;
use anyhow::Result;
use shellfox_runtime::{Command, Identity, Request};
impl Service {
    pub(crate) fn retire_remote_observer(
        &mut self,
        identity: Identity,
        observer: shellfox_runtime::ObserverLease,
    ) -> Result<()> {
        self.remote_v2_mailboxes.revoke_delivery(identity, observer);
        self.pump_remote_observer_retirements()
    }
    pub(crate) fn pump_remote_observer_retirements(&mut self) -> Result<()> {
        self.pump_manager_retirements()?;
        for (identity, observer) in self.remote_v2_mailboxes.retirement_candidates() {
            let current = self
                .terminals
                .get(&identity.tab_id)
                .is_some_and(|terminal| terminal.runtime.identity() == identity);
            if !current {
                self.remote_v2_mailboxes.source_destroyed(identity);
                continue;
            }
            let id = self.request_id()?;
            let request = Request {
                id,
                command: Command::Unobserve { observer },
            };
            let Some(terminal) = self.terminals.get(&identity.tab_id) else {
                continue;
            };
            match terminal.runtime.try_send(request) {
                Ok(()) => self
                    .remote_v2_mailboxes
                    .retirement_sent(identity, observer, id),
                Err(shellfox_runtime::SubmitError::Full(_)) => {} // retain revoked custody and retry next owner tick
                Err(shellfox_runtime::SubmitError::Disconnected(_)) => {
                    anyhow::bail!("Observer queue disconnected; revoked retirement retained until authoritative source removal.");
                }
                Err(shellfox_runtime::SubmitError::Invalid(error)) => {
                    return Err(anyhow::anyhow!(
                        "Observer retirement was not admitted: {error}"
                    ));
                }
            }
        }
        Ok(())
    }
}
