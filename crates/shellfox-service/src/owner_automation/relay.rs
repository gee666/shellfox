use super::*;
impl AutomationState {
    pub(super) fn pump(
        &mut self,
        terminals: &HashMap<TabId, terminals::OwnedTerminal>,
        stopping: bool,
    ) -> Result<()> {
        let mut first = self.pump_startup(stopping).err();
        for (id, relay) in &mut self.relays {
            if relay.failed_start {
                continue;
            }
            let result = terminals
                .get(&relay.feed.identity().tab_id)
                .context("Automation canonical parser owner is missing.")
                .and_then(|terminal| relay.feed.observe_runtime(&terminal.runtime))
                .and_then(|()| relay.feed.pump());
            if let Err(error) = result {
                if first.is_none() {
                    first = Some(error);
                }
            }
            let normal_exit = self.runs.is_stopped(*id).unwrap_or(false)
                && terminals
                    .get(&relay.feed.identity().tab_id)
                    .is_some_and(|terminal| relay.feed.finished(&terminal.runtime));
            if normal_exit || self.runs.failed_source_cleanup(*id).is_some() {
                let _ = relay.activity.raw_exit_observed(Instant::now());
            }
            if stopping || self.requested_stops.contains(id) {
                if let Some(failure) = relay.feed.failure() {
                    if let Err(error) = self.runs.begin_failed_source_cleanup(*id, failure) {
                        if first.is_none() {
                            first = Some(error);
                        }
                    }
                }
            }
        }
        if let Err(error) = self.runs.pump() {
            if first.is_none() {
                first = Some(error);
            }
        }
        let writes = std::mem::take(&mut self.writes);
        for pending in writes {
            match pending.receipt.try_result() {
                Ok(None) if !pending.reply.is_closed() => self.writes.push(pending),
                Ok(None) => {} // The OS write remains owned; no replay is implied.
                Ok(Some(())) => {
                    let _ = pending
                        .reply
                        .send(encode(AutomationAccepted { accepted: true }));
                }
                Err(error) => {
                    let _ = pending.reply.send(Err(error));
                }
            }
        }
        let stops = std::mem::take(&mut self.stops);
        for pending in stops {
            if let Some(proof) = self.runs.failed_source_cleanup(pending.id) {
                let _ = pending
                    .reply
                    .send(Err(AutomationViewFailure { proof }.into()));
                self.requested_stops.remove(&pending.id);
                continue;
            }
            let result = self.runs.is_stopped(pending.id).map(|stopped| {
                stopped
                    && self.relays.get(&pending.id).is_some_and(|relay| {
                        terminals
                            .get(&relay.feed.identity().tab_id)
                            .is_some_and(|terminal| relay.feed.finished(&terminal.runtime))
                    })
            });
            match result {
                Ok(false) if !pending.reply.is_closed() => self.stops.push(pending),
                Ok(false) => {} // Stop continues under the actual raw owner.
                Ok(true) => {
                    self.requested_stops.remove(&pending.id);
                    let _ = pending
                        .reply
                        .send(encode(AutomationStopped { stopped: true }));
                }
                Err(error) => {
                    let _ = pending.reply.send(Err(error));
                }
            }
        }
        for relay in self.relays.values() {
            if !terminals.contains_key(&relay.feed.identity().tab_id) && first.is_none() {
                first = Some(anyhow::anyhow!(
                    "Automation authoritative parser owner is missing."
                ));
            }
        }
        if let Some(error) = first {
            return Err(error);
        }
        Ok(())
    }
}
fn encode(value: impl serde::Serialize) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(value)?)
}
