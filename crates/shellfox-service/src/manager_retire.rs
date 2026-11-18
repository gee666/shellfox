//! Lease-only retirement. Revoked custody survives bounded queue pressure.
use crate::{manager_delivery::Role, Service};
use anyhow::Result;
use shellfox_runtime::{Command, Request};
impl Service {
    pub(crate) fn pump_manager_retirements(&mut self) -> Result<()> {
        for (identity, role) in self.manager_v2_mailboxes.retirements() {
            if !self
                .terminals
                .get(&identity.tab_id)
                .is_some_and(|terminal| terminal.runtime.identity() == identity)
            {
                self.manager_v2_mailboxes.source_destroyed(identity);
                continue;
            }
            let id = self.request_id()?;
            let command = match role {
                Role::Controller(lease) => Command::ReleaseControl { lease },
                Role::Observer(observer) => Command::Unobserve { observer },
            };
            let Some(terminal) = self.terminals.get(&identity.tab_id) else {
                continue;
            };
            match terminal.runtime.try_send(Request{id,command}){
                Ok(())=>self.manager_v2_mailboxes.sent(id,identity,role),
                Err(shellfox_runtime::SubmitError::Full(_))=>{},
                Err(shellfox_runtime::SubmitError::Disconnected(_))=>anyhow::bail!("Manager retirement disconnected; revoked custody retained until authoritative source removal."),
                Err(shellfox_runtime::SubmitError::Invalid(error))=>return Err(anyhow::anyhow!("Manager retirement was not admitted: {error}")),
            }
        }
        Ok(())
    }
}
