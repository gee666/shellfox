use super::*;
#[derive(Default)]
pub(super) struct Registry {
    pub attempts: HashMap<String, Attempt>,
    pub withdrawn: HashMap<String, u64>,
    pub views: HashMap<wire::Id, ObserverView>,
}
pub(super) struct Attempt {
    pub offer: wire::PairOffer,
    pub approval_client: ClientId,
    pub touched: u64,
    pub pairing: bool,
    pub connection: Option<ObserverConnection>,
}
pub(super) struct ObserverView {
    pub connection: ObserverConnection,
    pub polling: bool,
    pub operations: u8,
    pub touched: u64,
}
pub(super) fn attempt_key(peer: &str, client: wire::Id, attempt: wire::Id) -> String {
    format!("{peer}/{client}/{attempt}")
}
/// Drop on cancellation/timeout too, not only after a returned HTTP response.
pub(super) struct Slot {
    registry: Arc<Mutex<Registry>>,
    id: wire::Id,
    polling: bool,
}
impl Slot {
    pub fn enter(
        registry: &Arc<Mutex<Registry>>,
        connection: &ObserverConnection,
        polling: bool,
        now: u64,
    ) -> Result<Self, wire::Error> {
        let id = connection.binding().connection_id;
        let mut state = registry.lock().map_err(|_| application(invalid()))?;
        let view = state
            .views
            .get_mut(&id)
            .ok_or_else(|| application(auth()))?;
        if (polling && view.polling) || (!polling && view.operations >= 8) {
            return Err(application(invalid()));
        }
        if polling {
            view.polling = true;
        } else {
            view.operations += 1;
        }
        view.touched = now;
        Ok(Self {
            registry: registry.clone(),
            id,
            polling,
        })
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.lock() {
            if let Some(view) = state.views.get_mut(&self.id) {
                if self.polling {
                    view.polling = false;
                } else {
                    view.operations = view.operations.saturating_sub(1);
                }
            }
        }
    }
}
impl Backend {
    pub(super) fn observer_connection(
        &self,
        observer: &ObserverBackend,
        peer: &PeerIdentity,
        binding: &wire::Binding,
    ) -> Result<ObserverConnection, wire::Error> {
        if binding.owner_id != observer.owner_id {
            return Err(application(auth()));
        }
        let connection = observer
            .state
            .lock()
            .map_err(|_| application(invalid()))?
            .views
            .get(&binding.connection_id)
            .map(|view| view.connection.clone())
            .ok_or_else(|| application(auth()))?;
        let actual = AuthenticatedPeerId::new(peer.id.clone()).map_err(|_| application(auth()))?;
        connection.check(&actual, binding).map_err(application)?;
        Ok(connection)
    }
}
