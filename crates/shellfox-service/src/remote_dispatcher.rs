use super::RemoteState;
use crate::{OwnerDispatcher, OwnerWake, OwnerWakeSlot, Service};
use shellfox_remote::OwnerReceiver;

/// Drained one request at a time by the existing service ownership loop.
pub struct RemoteDispatcher {
    receiver: OwnerReceiver,
    state: RemoteState,
    facts: Option<super::facts::FactsPipeline>,
    wake: OwnerWakeSlot,
}
impl RemoteDispatcher {
    pub fn new(receiver: OwnerReceiver, state: RemoteState) -> Self {
        Self {
            receiver,
            state,
            facts: None,
            wake: OwnerWakeSlot::default(),
        }
    }
    /// The slot the request channel notifies (see owner_channel_with_wake).
    pub fn with_wake(mut self, wake: OwnerWakeSlot) -> Self {
        self.wake = wake;
        self
    }
    pub fn with_facts_provider(
        mut self,
        provider: std::sync::Arc<dyn super::ManagerFactsProvider>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        self.facts = Some(super::facts::FactsPipeline::new(provider, runtime));
        self
    }
}
impl OwnerDispatcher for RemoteDispatcher {
    /// Handles every finished fact job and queued request, then wakes the
    /// long-polls whose next Poll has events.
    fn dispatch_one(&mut self, service: &mut Service) {
        if let Some(facts) = &mut self.facts {
            while facts.finish(service, &mut self.state) {}
        }
        while let Ok(request) = self.receiver.try_recv() {
            match &mut self.facts {
                Some(facts)
                    if super::facts::requires_facts(&request.command, |tab| {
                        service.terminals.contains_key(&tab)
                    }) =>
                {
                    facts.admit(service, request, self.wake.clone())
                }
                _ => self.state.dispatch_request(service, request),
            }
        }
        self.state.wake_ready_views(service);
    }
    fn bind_wake(&mut self, wake: OwnerWake) {
        self.wake.bind(wake);
    }
}
