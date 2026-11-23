use super::*;
impl Service {
    pub(crate) fn confirm_external_cleanup(&self, tab: TabId) -> Result<()> {
        let terminal = self
            .terminals
            .get(&tab)
            .context("External parser owner is missing.")?;
        if let Some(proof) = terminal.external_cleanup {
            if proof.raw_cleanup_confirmed()
                && terminal.runtime.identity() == proof.runtime_identity()
                && terminal.runtime.external_status().is_some_and(|status| {
                    status.finished
                        && !status.failed
                        && status.source_sequence == proof.final_source_sequence()
                })
            {
                return Ok(());
            }
            bail!("Transferred external cleanup proof does not match its canonical parser.");
        }
        let (id, relay) = self
            .automation
            .relays
            .iter()
            .find(|(_, relay)| relay.feed.identity().tab_id == tab)
            .context("External parser has no retained raw source owner.")?;
        if relay.failed_start && !self.automation.runs.cleanup_pending(*id) {
            return Ok(());
        }
        let runtime = &self
            .terminals
            .get(&tab)
            .context("External parser owner is missing.")?
            .runtime;
        if !self.automation.runs.is_stopped(*id)? || !relay.feed.finished(runtime) {
            bail!("Raw cleanup and final external source drain are not both confirmed.");
        }
        Ok(())
    }
    pub(crate) fn close_external_parser(&mut self, identity: TerminalIdentity) -> Result<()> {
        self.confirm_external_cleanup(identity.tab_id)?;
        let runtime = &self
            .terminals
            .get(&identity.tab_id)
            .context("External parser owner is missing.")?
            .runtime;
        if runtime.identity().generation != identity.generation {
            bail!("External parser generation changed.");
        }
        let can_close = runtime
            .external_status()
            .is_some_and(|status| status.finished || status.failed);
        if can_close {
            // Source Finish can report Phase::Closed while the parser and its
            // command loop intentionally remain alive for late observers.
            runtime.request_external_close()?;
        } else {
            self.request_close(identity.tab_id)?;
        } // Cancelled, never-accepted raw startup.
        self.wait_until(
            Instant::now() + Duration::from_secs(12),
            |service| {
                service.drain(identity.tab_id)?;
                let runtime = &mut service
                    .terminals
                    .get_mut(&identity.tab_id)
                    .context("External parser owner disappeared.")?
                    .runtime;
                Ok(runtime.poll_external_closed()?.then_some(()))
            },
            || {
                anyhow::anyhow!(
                    "Original external parser-thread join is unconfirmed; owner retained."
                )
            },
        )?;
        self.activity_collect_live();
        self.forget_activity(
            self.terminals
                .get(&identity.tab_id)
                .context("External parser owner disappeared.")?
                .runtime
                .identity(),
        );
        let proof = self
            .terminals
            .get(&identity.tab_id)
            .and_then(|terminal| terminal.external_cleanup);
        let destroyed = self
            .terminals
            .get(&identity.tab_id)
            .context("External source disappeared before destruction proof.")?
            .runtime
            .identity();
        self.terminals.remove(&identity.tab_id);
        self.ownership_source_destroyed(destroyed);
        self.retire_external_source(identity.tab_id);
        if let Some(ClosedExternal::Closing(receipt)) =
            self.automation.closed_external.remove(&identity.tab_id)
        {
            self.automation
                .closed_external
                .insert(identity.tab_id, ClosedExternal::Joined(receipt));
        } else if let Some(proof) = proof {
            if !self.stopping
                && matches!(
                    self.host
                        .reserved_publication_receipt(proof.run_id().to_string().parse()?)?,
                    shellfox_host::ReservedPublicationReceipt::Published { .. }
                )
            {
                self.automation
                    .closed_external
                    .insert(identity.tab_id, ClosedExternal::Acknowledged(proof));
            }
        }
        self.ack_closed_external(identity.tab_id)
    }
    pub(crate) fn forget_closed_external(&mut self, tab: TabId) {
        self.automation.closed_external.remove(&tab);
    }
    pub(crate) fn retire_external_source(&mut self, tab: TabId) {
        self.automation
            .relays
            .retain(|_, relay| relay.feed.identity().tab_id != tab);
    }
    pub(crate) fn close_external_for_tab(&mut self, tab: TabId) -> Result<()> {
        if self.automation.closed_external.contains_key(&tab) && !self.terminals.contains_key(&tab)
        {
            return self.ack_closed_external(tab);
        }
        if !self
            .terminals
            .get(&tab)
            .is_some_and(|terminal| terminal.runtime.source_kind() == SourceKind::ExternalBytes)
        {
            return Ok(());
        }
        self.stop_external_for_tab(tab)?;
        if !self.terminals.contains_key(&tab) {
            return Ok(());
        } // Failed source evidence retains the independently joined owner proof.
        if self.automation.closed_external.len() >= AUTOMATION_RUNS
            && !self.automation.closed_external.contains_key(&tab)
        {
            bail!("Closed external proof retention bound reached; metadata retained.");
        }
        if let Some(id) = self
            .automation
            .relays
            .iter()
            .find(|(_, relay)| relay.feed.identity().tab_id == tab)
            .map(|(id, _)| *id)
        {
            let receipt = self.automation.runs.begin_retire(id)?;
            self.terminals
                .get_mut(&tab)
                .context("External parser owner is missing.")?
                .external_cleanup = Some(receipt.proof());
            self.automation
                .closed_external
                .insert(tab, ClosedExternal::Closing(receipt));
        }
        let generation = self
            .terminals
            .get(&tab)
            .context("External parser owner is missing.")?
            .runtime
            .identity()
            .generation;
        self.stop_terminal(TerminalIdentity {
            tab_id: tab,
            generation,
        })
    }
    fn stop_external_for_tab(&mut self, tab: TabId) -> Result<()> {
        if self
            .terminals
            .get(&tab)
            .is_some_and(|terminal| terminal.external_cleanup.is_some())
        {
            return self.confirm_external_cleanup(tab);
        }
        let Some(id) = self
            .automation
            .relays
            .iter()
            .find(|(_, relay)| relay.feed.identity().tab_id == tab)
            .map(|(id, _)| *id)
        else {
            return Ok(());
        };
        self.begin_automation_stop(AutomationStop { id })?;
        let failed_source = std::cell::Cell::new(false);
        self.wait_until(
            Instant::now() + Duration::from_secs(12),
            |service| {
                service.pump();
                if service.automation.runs.failed_source_cleanup(id).is_some() {
                    failed_source.set(true);
                    service.close_failed_automation_sources()?;
                    return Ok((!service.terminals.contains_key(&tab)).then_some(()));
                }
                failed_source.set(false);
                let relay = service
                    .automation
                    .relays
                    .get(&id)
                    .context("Automation source owner disappeared.")?;
                let terminal = service
                    .terminals
                    .get(&tab)
                    .context("Automation parser owner disappeared.")?;
                let stopped =
                    service.automation.runs.is_stopped(id)? && relay.feed.finished(&terminal.runtime);
                Ok(stopped.then_some(()))
            },
            || {
                anyhow::anyhow!(if failed_source.get() {
                    "Failed canonical parser original join is unconfirmed; metadata retained."
                } else {
                    "Automation stop/final source drain is unconfirmed; metadata and owners retained."
                })
            },
        )
    }
}
