use super::RemoteState;
use crate::{OwnerDispatcher, OwnerWake, OwnerWakeSlot, Service};
use shellfox_remote::{owner_channel_with_wake, OwnerClient, OwnerReceiver};

/// Capacity of the remote backend's request queue to the service owner.
const CAPACITY: usize = 32;

/// The owner's side of the remote request queue. Each enqueue wakes the owner.
pub struct RemoteInbox {
    receiver: OwnerReceiver,
    wake: OwnerWakeSlot,
}
/// The queue between the remote backend (first) and its dispatcher (second).
pub fn remote_channel() -> (OwnerClient, RemoteInbox) {
    let wake = OwnerWakeSlot::default();
    let (client, receiver) = owner_channel_with_wake(CAPACITY, wake.notifier());
    (client, RemoteInbox { receiver, wake })
}

/// Drained by the service owner loop after each pass.
pub struct RemoteDispatcher {
    receiver: OwnerReceiver,
    state: RemoteState,
    facts: Option<super::facts::FactsPipeline>,
    wake: OwnerWakeSlot,
}
impl RemoteDispatcher {
    pub fn new(inbox: RemoteInbox, state: RemoteState) -> Self {
        Self {
            receiver: inbox.receiver,
            state,
            facts: None,
            wake: inbox.wake,
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
impl RemoteDispatcher {
    /// Holds the request behind a close, or sends it to fact collection or dispatch.
    fn route(&mut self, service: &mut Service, request: shellfox_remote::OwnerRequest) {
        let Some(request) = self.state.hold(service, request) else {
            return;
        };
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
}
impl OwnerDispatcher for RemoteDispatcher {
    /// Handles every finished fact job and queued request, then wakes the
    /// long-polls whose next Poll has events.
    fn dispatch_one(&mut self, service: &mut Service) {
        // Answer a finished close before anything can start another one.
        self.state.finish_close(service);
        if let Some(facts) = &mut self.facts {
            while facts.finish(service, &mut self.state) {}
        }
        self.state.finish_stats(service);
        self.state.finish_close(service);
        // Held requests collect facts again: facts from before the commit are stale.
        while let Some(request) = self.state.next_held(service) {
            self.route(service, request);
        }
        while let Ok(request) = self.receiver.try_recv() {
            self.route(service, request);
        }
        // A close that finished during this dispatch is answered now.
        self.state.finish_close(service);
        self.state.wake_ready_views(service);
    }
    fn bind_wake(&mut self, wake: OwnerWake) {
        self.wake.bind(wake);
    }
}
