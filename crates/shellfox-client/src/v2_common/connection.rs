use super::{
    state::{ClientError, DiscoveryOutcome, FallbackPolicy, Guard, Kind, Status},
    CommonClient, Profile, WireReply,
};
use serde::Deserialize;
use shellfox_protocol::{observer_v2 as o, *};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyDiscovery {
    service: o::ServiceName,
    protocol_version: u8,
}
impl Validate for LegacyDiscovery {
    fn validate(&self) -> Result<(), ValidationError> {
        validation::ensure(self.service == o::ServiceName::Shellfox && self.protocol_version == 1)
    }
}
impl<T: OutboundTransport, P: Profile> CommonClient<T, P> {
    pub async fn discover(
        &self,
        policy: FallbackPolicy,
    ) -> Result<DiscoveryOutcome, ClientError<P::Error>> {
        let (cycle, cancel, may_fallback) = {
            let mut s = self.state.lock().expect("v2 state");
            if s.binding.is_some() || s.negotiating || s.polling || s.pending > 0 {
                return Err(ClientError::Busy);
            }
            s.negotiating = true;
            s.discovery = None;
            s.status = Status::Disconnected;
            (
                s.cycle,
                s.cancel.child_token(),
                !s.pair_started && policy == FallbackPolicy::OfferLegacy,
            )
        };
        let _guard = Guard {
            state: &self.state,
            cycle,
            kind: Kind::Negotiate,
            cancel: cancel.clone(),
        };
        let response = self
            .send::<_, P::Reply<o::Discovery>>(
                Endpoint::DiscoverV2,
                &o::Discover {},
                cancel.clone(),
            )
            .await
            .map(WireReply::result);
        let fallback = may_fallback
            && match &response {
                Err(ClientError::Transport(TransportError::Rejected { status: 404 })) => true,
                Ok(Ok(value)) => !P::supports(value),
                Ok(Err(error)) => P::incompatible(error),
                _ => false,
            };
        if fallback {
            let legacy = self
                .send::<_, Reply<LegacyDiscovery>>(Endpoint::Discover, &o::Discover {}, cancel)
                .await;
            let mut s = self.state.lock().expect("v2 state");
            if s.cycle != cycle {
                return Err(ClientError::StaleConnection);
            }
            if !matches!(legacy?, Reply::Success(_)) {
                return Err(ClientError::InvalidResponse);
            }
            s.discovery = None;
            s.status = Status::LegacyAvailable;
            return Ok(DiscoveryOutcome::LegacyAvailable);
        }
        let mut s = self.state.lock().expect("v2 state");
        if s.cycle != cycle {
            return Err(ClientError::StaleConnection);
        }
        match response? {
            Ok(value) => {
                if P::supports(&value) {
                    s.discovery = Some(value.clone());
                    s.status = Status::Discovered;
                    Ok(DiscoveryOutcome::Available(value))
                } else {
                    Ok(DiscoveryOutcome::Incompatible(value))
                }
            }
            Err(error) => Err(ClientError::Host(error)),
        }
    }
    pub async fn pair(
        &self,
        attempt: o::Id,
    ) -> Result<Result<P::Selection, P::Error>, ClientError<P::Error>> {
        self.pair_with_offer(attempt, Ok)
            .await
            .map(|result| result.map(|(selection, _)| selection))
    }
    pub async fn pair_with_offer(
        &self,
        attempt: o::Id,
        select: impl FnOnce(P::Offer) -> Result<P::Offer, ClientError<P::Error>>,
    ) -> Result<
        Result<(P::Selection, super::BindingWitness<P::Binding>), P::Error>,
        ClientError<P::Error>,
    > {
        let (offer, cycle, cancel) = {
            let mut s = self.state.lock().expect("v2 state");
            if s.negotiating || s.polling || s.pending > 0 || s.binding.is_some() {
                return Err(ClientError::Busy);
            }
            let owner = s
                .discovery
                .as_ref()
                .ok_or(ClientError::NotDiscovered)?
                .owner_id;
            let offer = select(P::offer(s.client_id, attempt, owner))?;
            s.negotiating = true;
            s.pair_started = true;
            s.attempt = Some(attempt);
            s.status = Status::Connecting;
            (offer, s.cycle, s.cancel.child_token())
        };
        let _guard = Guard {
            state: &self.state,
            cycle,
            kind: Kind::Negotiate,
            cancel: cancel.clone(),
        };
        let response = self
            .send::<_, P::Reply<P::Selection>>(Endpoint::PairV2, &offer, cancel)
            .await
            .map(WireReply::result);
        let mut s = self.state.lock().expect("v2 state");
        if s.cycle != cycle {
            return Err(ClientError::StaleConnection);
        }
        match response {
            Ok(Ok(selection)) => {
                let binding = match P::selection(&selection, &offer) {
                    Ok(binding) => binding,
                    Err(_) => {
                        s.invalidate(Status::Rejected);
                        return Err(ClientError::InvalidResponse);
                    }
                };
                s.binding = Some(binding.clone());
                s.status = Status::Connected;
                // Issue the witness for THIS PAIR under the same actual state lock.
                let witness = super::BindingWitness {
                    state: self.state.clone(),
                    binding,
                    cycle,
                };
                Ok(Ok((selection, witness)))
            }
            Ok(Err(error)) => {
                s.status = match P::code(&error) {
                    ErrorCode::ApprovalRequired => Status::AwaitingApproval,
                    ErrorCode::AccessDenied => Status::Rejected,
                    _ => Status::Reconnecting,
                };
                Ok(Err(error))
            }
            Err(error) => {
                s.status = Status::Reconnecting;
                Err(error)
            }
        }
    }

    pub async fn disconnect(
        &self,
    ) -> Result<Result<o::Disconnected, P::Error>, ClientError<P::Error>> {
        let request = {
            let mut s = self.state.lock().expect("v2 state");
            let request = P::disconnect(s.binding.clone(), s.client_id, s.attempt)
                .ok_or(ClientError::NotConnected)?;
            s.invalidate(Status::Disconnected);
            s.discovery = None;
            s.attempt = None;
            request
        };
        self.send::<_, P::Reply<o::Disconnected>>(
            Endpoint::DisconnectV2,
            &request,
            CancellationToken::new(),
        )
        .await
        .map(WireReply::result)
    }
}
