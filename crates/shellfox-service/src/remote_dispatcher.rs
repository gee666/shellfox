use super::RemoteState;
use crate::{OwnerDispatcher, Service};
use shellfox_remote::OwnerReceiver;

/// Drained one request at a time by the existing service ownership loop.
pub struct RemoteDispatcher {
    receiver: OwnerReceiver,
    state: RemoteState,
    facts: Option<super::facts::FactsPipeline>,
}
impl RemoteDispatcher {
    pub fn new(receiver: OwnerReceiver, state: RemoteState) -> Self {
        Self {
            receiver,
            state,
            facts: None,
        }
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
    fn dispatch_one(&mut self, service: &mut Service) {
        if let Some(facts) = &mut self.facts {
            if facts.finish(service, &mut self.state) {
                return;
            }
        }
        if let Ok(request) = self.receiver.try_recv() {
            if super::facts::requires_facts(&request.command, |tab| service.terminals.contains_key(&tab)) {
                if let Some(facts) = &mut self.facts {
                    facts.admit(service, request);
                    return;
                }
            }
            self.state.dispatch_request(service, request);
        }
    }
}
