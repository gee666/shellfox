use super::{ClientError, DiscoveryOutcome, FallbackPolicy, Status};
use crate::v2_common::{CommonClient, ManagerProfile};
use shellfox_protocol::{manager_v2 as m, *};
/// Opaque witness to the SAME current client state. Not a host grant issuer.
#[derive(Clone)]
pub struct ManagerBindingWitness {
    inner: crate::v2_common::BindingWitness<m::Binding>,
    selection: std::sync::Arc<m::PairSelection>,
}
impl ManagerBindingWitness {
    /// The authenticated selection issued with this exact binding/cycle.
    pub fn selection(&self) -> &m::PairSelection {
        &self.selection
    }
    /// Compare issued connection/cycle identity without recapturing current state.
    pub fn same_origin(&self, other: &Self) -> bool {
        self.inner.same_origin(&other.inner)
    }
    pub fn binding(&self) -> &m::Binding {
        self.inner.binding()
    }
    /// Lock order: actual client state, SDK recovery state, engine queue.
    /// Only bounded infallible local swaps/ticket commit may run under this
    /// fence. Drop retired engines and emit notifications AFTER it returns.
    pub fn commit_if_current<R>(&self, operation: impl FnOnce() -> R) -> Result<R, ClientError> {
        self.inner
            .commit(operation)
            .map_err(|_| ClientError::StaleConnection)
    }
}
/// No automatic Claim, takeover or observer fallback. References come from the
/// server's actual Runtime leases, not PairSelection permission metadata.
pub struct ManagerClient<T> {
    inner: CommonClient<T, ManagerProfile>,
    receiver: Option<m::Native0Codec>,
    negotiated: std::sync::Mutex<Option<ManagerBindingWitness>>,
}
impl<T: OutboundTransport> ManagerClient<T> {
    pub fn new(peer: AuthenticatedPeerId, client: m::Id, transport: T) -> Self {
        Self {
            inner: CommonClient::new(peer, client, transport),
            receiver: None,
            negotiated: std::sync::Mutex::new(None),
        }
    }
    /// Explicit Rust-only native recovery offer; no default/client engine switch.
    pub fn native0(
        peer: AuthenticatedPeerId,
        client: m::Id,
        transport: T,
        receiver: m::Native0Codec,
    ) -> Result<Self, ClientError> {
        receiver
            .validate()
            .map_err(|_| ClientError::InvalidRequest)?;
        Ok(Self {
            inner: CommonClient::new(peer, client, transport),
            receiver: Some(receiver),
            negotiated: std::sync::Mutex::new(None),
        })
    }
    /// Retained authenticated PAIR result, fenced by the current binding.
    pub fn selection(&self) -> Option<m::PairSelection> {
        self.binding_witness()
            .ok()
            .map(|origin| origin.selection().clone())
    }
    pub fn binding_witness(&self) -> Result<ManagerBindingWitness, ClientError> {
        let current = self.inner.binding_witness()?;
        current
            .commit(|| {
                let retained = self
                    .negotiated
                    .lock()
                    .map_err(|_| ClientError::InvalidResponse)?;
                retained
                    .as_ref()
                    .filter(|origin| origin.inner.same_origin(&current))
                    .cloned()
                    .ok_or(ClientError::StaleConnection)
            })
            .map_err(|_| ClientError::StaleConnection)?
    }
    pub fn status(&self) -> Status {
        self.inner.status()
    }
    pub fn binding(&self) -> Option<m::Binding> {
        self.inner.binding()
    }
    pub fn peer(&self) -> &AuthenticatedPeerId {
        self.inner.peer()
    }
    pub fn reconnect(&self, client: m::Id) {
        // Stale cached selection cannot pass binding_witness's actual-state fence.
        self.inner.reconnect(client)
    }
    pub async fn discover(&self, policy: FallbackPolicy) -> Result<DiscoveryOutcome, ClientError> {
        self.inner.discover(policy).await
    }
    pub async fn pair(&self, attempt: m::Id) -> Result<m::Reply<m::PairSelection>, ClientError> {
        let receiver = self.receiver.clone();
        let result = self
            .inner
            .pair_with_offer(attempt, move |mut offer| {
                if let Some(receiver) = receiver {
                    offer.recovery_format = m::RecoveryFormat::Native0RawV1;
                    offer.native0_receiver = Some(receiver);
                }
                offer.validate().map_err(|_| ClientError::InvalidRequest)?;
                Ok(offer)
            })
            .await?;
        Ok(match result {
            Ok((v, inner)) => {
                let origin = ManagerBindingWitness {
                    inner,
                    selection: std::sync::Arc::new(v.clone()),
                };
                origin.commit_if_current(|| {
                    *self
                        .negotiated
                        .lock()
                        .map_err(|_| ClientError::InvalidResponse)? = Some(origin.clone());
                    Ok::<(), ClientError>(())
                })??;
                m::Reply::Success(v)
            }
            Err(e) => m::Reply::Failure(e),
        })
    }
    pub async fn execute<R: m::Request>(
        &self,
        request: &R,
    ) -> Result<m::Reply<R::Response>, ClientError> {
        request
            .validate_wire()
            .map_err(|_| ClientError::InvalidRequest)?;
        Ok(match self.inner.request(R::METHOD, request).await? {
            Ok(value) => {
                if request.validate_response(&value).is_err() {
                    self.inner.invalidate_response();
                    return Err(ClientError::InvalidResponse);
                }
                m::Reply::Success(value)
            }
            Err(error) => m::Reply::Failure(error),
        })
    }
    /// Same actual binding/cycle must still hold when the request is admitted.
    pub async fn execute_with_witness<R: m::Request>(
        &self,
        witness: &ManagerBindingWitness,
        request: &R,
    ) -> Result<m::Reply<R::Response>, ClientError> {
        request
            .validate_wire()
            .map_err(|_| ClientError::InvalidRequest)?;
        Ok(
            match self
                .inner
                .request_with_witness(R::METHOD, request, &witness.inner)
                .await?
            {
                Ok(value) => {
                    if request.validate_response(&value).is_err() {
                        self.inner.invalidate_response();
                        return Err(ClientError::InvalidResponse);
                    }
                    m::Reply::Success(value)
                }
                Err(error) => m::Reply::Failure(error),
            },
        )
    }
    pub async fn poll(&self) -> Result<m::Reply<m::PollResponse>, ClientError> {
        Ok(match self.inner.poll().await? {
            Ok(v) => m::Reply::Success(v),
            Err(e) => m::Reply::Failure(e),
        })
    }
    pub async fn poll_with_witness(
        &self,
        witness: &ManagerBindingWitness,
    ) -> Result<m::Reply<m::PollResponse>, ClientError> {
        Ok(match self.inner.poll_with_witness(&witness.inner).await? {
            Ok(value) => m::Reply::Success(value),
            Err(error) => m::Reply::Failure(error),
        })
    }
    pub async fn disconnect(&self) -> Result<m::Reply<m::Disconnected>, ClientError> {
        Ok(match self.inner.disconnect().await? {
            Ok(v) => m::Reply::Success(v),
            Err(e) => m::Reply::Failure(e),
        })
    }
}
