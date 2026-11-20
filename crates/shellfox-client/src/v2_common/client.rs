use super::{
    state::{ClientError, Guard, Kind, State, Status},
    Profile, WireBoundReply,
};
use serde::{de::DeserializeOwned, Serialize};
use shellfox_protocol::{observer_v2::Id, *};
use std::sync::{Arc, Mutex};
/// Same actual state mutex, not a copied status or a second cancellation ledger.
#[derive(Clone)]
pub(crate) struct BindingWitness<B> {
    pub(super) state: Arc<Mutex<State<B>>>,
    pub(super) binding: B,
    pub(super) cycle: u64,
}
impl<B: Clone + PartialEq> BindingWitness<B> {
    pub(crate) fn same_origin(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
            && self.cycle == other.cycle
            && self.binding == other.binding
    }
    pub(crate) fn binding(&self) -> &B {
        &self.binding
    }
    pub(crate) fn commit<R>(&self, operation: impl FnOnce() -> R) -> Result<R, ()> {
        let state = self.state.lock().map_err(|_| ())?;
        if state.cycle != self.cycle
            || state.status != Status::Connected
            || state.cancel.is_cancelled()
            || state.binding.as_ref() != Some(&self.binding)
        {
            return Err(());
        }
        // Caller permits only bounded infallible local swaps/ticket commit.
        // No await, IO, callback, notification or old-engine destruction here.
        Ok(operation())
    }
}
pub(crate) struct CommonClient<T, P: Profile> {
    pub(super) transport: T,
    pub(super) peer: AuthenticatedPeerId,
    pub(super) state: Arc<Mutex<State<P::Binding>>>,
}
impl<T: OutboundTransport, P: Profile> CommonClient<T, P> {
    pub fn new(peer: AuthenticatedPeerId, client_id: Id, transport: T) -> Self {
        Self {
            transport,
            peer,
            state: Arc::new(Mutex::new(State {
                client_id,
                cycle: 0,
                cancel: CancellationToken::new(),
                discovery: None,
                binding: None,
                attempt: None,
                pair_started: false,
                pending: 0,
                polling: false,
                negotiating: false,
                status: Status::Disconnected,
            })),
        }
    }
    pub(crate) fn binding_witness(
        &self,
    ) -> Result<BindingWitness<P::Binding>, ClientError<P::Error>> {
        let state = self
            .state
            .lock()
            .map_err(|_| ClientError::InvalidResponse)?;
        if state.status != Status::Connected || state.cancel.is_cancelled() {
            return Err(ClientError::NotConnected);
        }
        Ok(BindingWitness {
            state: self.state.clone(),
            binding: state.binding.clone().ok_or(ClientError::NotConnected)?,
            cycle: state.cycle,
        })
    }
    pub fn invalidate_response(&self) {
        self.state
            .lock()
            .expect("v2 state")
            .invalidate(Status::Reconnecting);
    }
    pub fn status(&self) -> Status {
        self.state.lock().expect("v2 state").status
    }
    pub fn binding(&self) -> Option<P::Binding> {
        self.state.lock().expect("v2 state").binding.clone()
    }
    pub fn peer(&self) -> &AuthenticatedPeerId {
        &self.peer
    }
    pub fn reconnect(&self, client: Id) {
        let mut s = self.state.lock().expect("v2 state");
        s.invalidate(Status::Disconnected);
        s.client_id = client;
        s.discovery = None;
        s.attempt = None;
    }
    pub async fn request<R: Serialize, V: DeserializeOwned + Validate>(
        &self,
        method: &'static str,
        payload: &R,
    ) -> Result<Result<V, P::Error>, ClientError<P::Error>> {
        self.request_selected(method, payload, None).await
    }
    pub(crate) async fn request_with_witness<R: Serialize, V: DeserializeOwned + Validate>(
        &self,
        method: &'static str,
        payload: &R,
        witness: &BindingWitness<P::Binding>,
    ) -> Result<Result<V, P::Error>, ClientError<P::Error>> {
        self.request_selected(method, payload, Some(witness)).await
    }
    async fn request_selected<R: Serialize, V: DeserializeOwned + Validate>(
        &self,
        method: &'static str,
        payload: &R,
        witness: Option<&BindingWitness<P::Binding>>,
    ) -> Result<Result<V, P::Error>, ClientError<P::Error>> {
        let (binding, cycle, cancel) = {
            let mut s = self.state.lock().expect("v2 state");
            let binding = s.binding.clone().ok_or(ClientError::NotConnected)?;
            if let Some(witness) = witness {
                if !Arc::ptr_eq(&self.state, &witness.state)
                    || s.cycle != witness.cycle
                    || s.status != Status::Connected
                    || s.cancel.is_cancelled()
                    || binding != witness.binding
                {
                    return Err(ClientError::StaleConnection);
                }
            }
            if s.pending >= 8 {
                return Err(ClientError::Busy);
            }
            s.pending += 1;
            (binding, s.cycle, s.cancel.child_token())
        };
        let _guard = Guard {
            state: &self.state,
            cycle,
            kind: Kind::Request,
            cancel: cancel.clone(),
        };
        #[derive(Serialize)]
        struct Envelope<'a, B, R> {
            version: observer_v2::Version,
            binding: &'a B,
            method: &'static str,
            payload: &'a R,
        }
        let reply = self
            .send::<_, P::Bound<V>>(
                Endpoint::RequestV2,
                &Envelope {
                    version: observer_v2::Version,
                    binding: &binding,
                    method,
                    payload,
                },
                cancel,
            )
            .await;
        self.finish_bound(reply, &binding, cycle, method)
    }
    pub async fn poll(&self) -> Result<Result<P::Poll, P::Error>, ClientError<P::Error>> {
        self.poll_selected(None).await
    }
    pub(crate) async fn poll_with_witness(
        &self,
        witness: &BindingWitness<P::Binding>,
    ) -> Result<Result<P::Poll, P::Error>, ClientError<P::Error>> {
        self.poll_selected(Some(witness)).await
    }
    async fn poll_selected(
        &self,
        witness: Option<&BindingWitness<P::Binding>>,
    ) -> Result<Result<P::Poll, P::Error>, ClientError<P::Error>> {
        let (binding, cycle, cancel) = {
            let mut s = self.state.lock().expect("v2 state");
            let binding = s.binding.clone().ok_or(ClientError::NotConnected)?;
            if witness.is_some_and(|w| {
                !Arc::ptr_eq(&self.state, &w.state)
                    || s.cycle != w.cycle
                    || s.status != Status::Connected
                    || s.cancel.is_cancelled()
                    || binding != w.binding
            }) {
                return Err(ClientError::StaleConnection);
            }
            if s.polling {
                return Err(ClientError::Busy);
            }
            s.polling = true;
            (binding, s.cycle, s.cancel.child_token())
        };
        let _guard = Guard {
            state: &self.state,
            cycle,
            kind: Kind::Poll,
            cancel: cancel.clone(),
        };
        #[derive(Serialize)]
        struct Poll<B> {
            version: observer_v2::Version,
            binding: B,
        }
        let reply = self
            .send::<_, P::Bound<P::Poll>>(
                Endpoint::EventsV2,
                &Poll {
                    version: observer_v2::Version,
                    binding: binding.clone(),
                },
                cancel,
            )
            .await;
        self.finish_bound(reply, &binding, cycle, "events")
    }
    fn finish_bound<V: DeserializeOwned + Validate>(
        &self,
        result: Result<P::Bound<V>, ClientError<P::Error>>,
        expected: &P::Binding,
        cycle: u64,
        method: &str,
    ) -> Result<Result<V, P::Error>, ClientError<P::Error>> {
        let mut s = self.state.lock().expect("v2 state");
        if s.cycle != cycle {
            return Err(ClientError::StaleConnection);
        }
        let result = result.and_then(|value| {
            let (binding, outcome) = value.parts();
            if binding != *expected {
                return Err(ClientError::InvalidResponse);
            }
            Ok(outcome)
        });
        match &result {
            Ok(Err(error)) if crate::client::is_connection_denial(method, &P::code(error)) => {
                let status = match P::code(error) {
                    ErrorCode::AccessDenied => Status::Rejected,
                    ErrorCode::ApprovalRequired => Status::AwaitingApproval,
                    _ => Status::Reconnecting,
                };
                s.invalidate(status);
            }
            Err(
                ClientError::Transport(TransportError::Cancelled) | ClientError::InvalidRequest,
            ) => {}
            Err(_) => s.invalidate(Status::Reconnecting),
            _ => {}
        }
        result
    }
}
