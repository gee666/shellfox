use super::*;
use shellfox_host::ReservedPublicationReceipt;
use shellfox_runtime::{ExternalDisplayPolicy, ExternalStartStage, Runtime};
impl Service {
    pub(super) fn start_automation(
        &mut self,
        input: AutomationStart,
        controller: u64,
    ) -> Result<AutomationRun> {
        self.assert_accepting()?;
        if let Some(error) = &self.automation.fault {
            bail!("{error}");
        }
        let id = input.request_id;
        if self
            .automation
            .startup
            .iter()
            .any(|pending| pending.id == id)
        {
            bail!(
                "The original external parser startup still owns cleanup; no raw target was launched."
            );
        }
        let exists = self
            .automation
            .runs
            .list()?
            .0
            .iter()
            .any(|run| run.id == id);
        if !exists {
            if self.automation.relays.contains_key(&id) {
                bail!(
                    "The original automation source still owns cleanup; request ID was not reused."
                );
            }
            if !matches!(
                self.host
                    .reserved_publication_receipt(id.to_string().parse()?)?,
                ReservedPublicationReceipt::ReceiptUnavailable
            ) {
                bail!(
                    "Request ID has an existing durable automation publication; no new raw target was launched."
                );
            }
        }
        if let Some(run) = self.automation.runs.reserve_start(&input, controller)? {
            return Ok(run);
        }
        self.pump_automation_retirement()?;
        self.free_hidden_automation_slot()?;
        let receiver = self.automation.runs.take_source(id)?;
        let identity = Identity {
            session_id: shellfox_core::SessionId::new(),
            tab_id: TabId::new(),
            generation: shellfox_core::Generation::new(),
        };
        let activity = std::sync::Arc::new(AutomationActivity::new(identity));
        let start = || {
            Runtime::start_external_with_policy_and_hooks(
                identity,
                Default::default(),
                ExternalDisplayPolicy::LegacyAutomationV1,
                activity.hooks(),
            )
        };
        let started = start();
        let mut runtime = match started {
            Ok(runtime) => runtime,
            Err(error) => {
                match error.stage {
                    ExternalStartStage::BeforeThread => {
                        self.automation.runs.cancel_reserved(id, controller)?
                    }
                    ExternalStartStage::OwnerUnconfirmed => {
                        let mut owner = error.take_owner().context(
                            "External parser readiness failed without its original cleanup owner.",
                        )?;
                        owner.request_cancel();
                        self.automation.startup.push(PendingStartup {
                            id,
                            controller,
                            owner,
                        });
                    }
                }
                return Err(error.into());
            }
        };
        self.automation.runs.bind_source_identity(id, identity)?;
        let feed = AutomationRelay::new(receiver, &mut runtime)?;
        self.install_terminal(identity.tab_id, terminals::OwnedTerminal::new(runtime));
        self.automation.relays.insert(
            id,
            Relay {
                feed,
                failed_start: false,
                activity,
            },
        );
        let binding = self
            .automation
            .relays
            .get_mut(&id)
            .context("Canonical automation source disappeared before raw launch.")?
            .feed
            .take_launch_binding()
            .context("Canonical automation launch binding was already consumed.")?;
        let result = self
            .automation
            .runs
            .start_with_launch_binding(input, controller, binding);
        if result.is_ok() {
            let evidence = self
                .automation
                .runs
                .take_launch_evidence(id)?
                .context("Ready raw launch supplied no sealed launch receipt.")?;
            self.automation
                .relays
                .get_mut(&id)
                .context("Canonical automation source disappeared after READY.")?
                .feed
                .bind_launch_evidence(evidence)?;
        }
        if result.is_err() {
            if let Some(relay) = self.automation.relays.get_mut(&id) {
                relay.failed_start = true;
                relay
                    .feed
                    .abort_source(crate::automation::AutomationSourceAbortReason::RawSpawnFailed)?;
            }
        }
        result
    }
}
impl AutomationState {
    pub(super) fn pump_startup(&mut self, stopping: bool) -> Result<()> {
        let pending = std::mem::take(&mut self.startup);
        let mut first = None;
        for mut pending in pending {
            pending.owner.request_cancel();
            if pending.owner.poll_closed() {
                if !stopping {
                    if let Err(error) = self.runs.cancel_reserved(pending.id, pending.controller) {
                        if first.is_none() {
                            first = Some(error);
                        }
                    }
                }
            } else {
                self.startup.push(pending);
            }
        }
        if let Some(error) = first {
            return Err(error);
        }
        Ok(())
    }
}
