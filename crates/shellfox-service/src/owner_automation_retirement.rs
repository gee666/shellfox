use super::*;
use crate::automation::AutomationRetirementAck;
impl Service {
    pub(super) fn free_hidden_automation_slot(&mut self) -> Result<()> {
        let owned = self
            .terminals
            .values()
            .filter(|terminal| terminal.runtime.source_kind() == SourceKind::ExternalBytes)
            .count()
            + self.automation.startup.len();
        if owned < AUTOMATION_RUNS {
            return Ok(());
        }
        for run in self.automation.runs.list()?.0 {
            if run.session_id.is_some()
                || run.state != shellfox_protocol::automation::AutomationState::Exited
            {
                continue;
            }
            let finished = self.automation.relays.get(&run.id).is_some_and(|relay| {
                self.terminals
                    .get(&relay.feed.identity().tab_id)
                    .is_some_and(|terminal| relay.feed.finished(&terminal.runtime))
            });
            if finished
                && self.automation.runs.is_stopped(run.id)?
                && self.automation.runs.begin_retire(run.id).is_ok()
            {
                self.pump_automation_retirement()?;
                break;
            }
        }
        // If only published/unacknowledged sources remain, Runtime's reserved
        // cleanup slot fails BeforeThread; the caller cancels the new reservation.
        Ok(())
    }
    pub(super) fn ack_closed_external(&mut self, tab: TabId) -> Result<()> {
        let receipt = match self.automation.closed_external.get(&tab) {
            Some(ClosedExternal::Joined(receipt)) => *receipt,
            Some(ClosedExternal::Acknowledged(proof)) => {
                if proof.runtime_identity().tab_id != tab || !proof.raw_cleanup_confirmed() {
                    bail!("Closed external proof identity changed.");
                }
                return Ok(());
            }
            Some(ClosedExternal::Closing(_)) => {
                bail!("Original external parser join remains unconfirmed.")
            }
            None => return Ok(()),
        };
        let proof = receipt.proof();
        let identity = proof.runtime_identity();
        if identity.tab_id != tab || self.terminals.contains_key(&tab) {
            bail!("External close cannot be acknowledged before original parser join.");
        }
        let ack = if receipt.publication().is_some() {
            AutomationRetirementAck::PublishedParserClosed { identity }
        } else {
            AutomationRetirementAck::UnshownParserClosed { identity }
        };
        self.automation.runs.ack_retired(proof, ack)?;
        self.automation
            .closed_external
            .insert(tab, ClosedExternal::Acknowledged(proof));
        Ok(())
    }
    pub(super) fn pump_automation_retirement(&mut self) -> Result<()> {
        for receipt in self.automation.runs.retired() {
            let proof = receipt.proof();
            let identity = proof.runtime_identity();
            if self
                .automation
                .closed_external
                .contains_key(&identity.tab_id)
            {
                if matches!(
                    self.automation.closed_external.get(&identity.tab_id),
                    Some(ClosedExternal::Closing(_))
                ) {
                    continue;
                }
                self.ack_closed_external(identity.tab_id)?;
                continue;
            }
            if !proof.raw_cleanup_confirmed() {
                bail!("Automation retirement lacks original raw cleanup proof.");
            }
            let relay = self
                .automation
                .relays
                .get(&proof.run_id())
                .context("Retired automation canonical source owner is missing.")?;
            let terminal = self
                .terminals
                .get(&identity.tab_id)
                .context("Retired automation canonical parser is missing.")?;
            if relay.feed.identity() != identity || terminal.runtime.identity() != identity {
                bail!("Automation retirement source identity/generation changed.");
            }
            if !relay.feed.finished(&terminal.runtime) {
                continue;
            }
            if terminal
                .runtime
                .external_status()
                .is_none_or(|status| status.source_sequence != proof.final_source_sequence())
            {
                bail!("Automation retirement final source sequence changed.");
            }
            self.terminals
                .get_mut(&identity.tab_id)
                .context("Retiring parser disappeared.")?
                .external_cleanup = Some(proof);
            let ack = if let Some(publication) = receipt.publication() {
                if publication.session_id.to_string() != identity.session_id.to_string()
                    || publication.tab_id.to_string() != identity.tab_id.to_string()
                {
                    bail!("Published retirement target changed.");
                }
                // The existing terminal owner now retains the sealed raw proof.
                // Keep the canonical parser and all observer commands alive.
                self.automation.relays.remove(&proof.run_id());
                AutomationRetirementAck::PublishedParserTransferred { identity }
            } else {
                self.stop_terminal(TerminalIdentity {
                    tab_id: identity.tab_id,
                    generation: identity.generation,
                })?;
                AutomationRetirementAck::UnshownParserClosed { identity }
            };
            self.automation.runs.ack_retired(proof, ack)?;
        }
        Ok(())
    }
}
