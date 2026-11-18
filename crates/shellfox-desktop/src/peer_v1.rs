//! A v1 view bound to one authenticated tailnet peer through the existing Go helper.
use anyhow::{anyhow, Result};
use shellfox_client::{Client, ClientState, ConnectionStatus};
use shellfox_protocol::{
    AuthenticatedPeerId, ClientId, Disconnected, Endpoint, GetSnapshot, ManagerSnapshot,
    OutboundTransport, PairResponse, Reply, Request, RequestLimits, TransportError,
    TransportFuture,
};
use shellfox_service::{native::NativeClient, remote::OutgoingV2Endpoint};

struct Transport(NativeClient);

impl OutboundTransport for Transport {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let endpoint = match endpoint {
                Endpoint::Discover => OutgoingV2Endpoint::DiscoverV1,
                Endpoint::Pair => OutgoingV2Endpoint::PairV1,
                Endpoint::Request => OutgoingV2Endpoint::RequestV1,
                Endpoint::Events => OutgoingV2Endpoint::EventsV1,
                Endpoint::Disconnect => OutgoingV2Endpoint::DisconnectV1,
                _ => return Err(TransportError::InvalidRequest),
            };
            let response = self
                .0
                .outgoing_v2_with_status(peer, endpoint, body, limits)
                .await?;
            if !(200..300).contains(&response.status) {
                return Err(TransportError::Rejected {
                    status: response.status,
                });
            }
            Ok(response.body)
        })
    }
}

pub struct V1Binding(Client<Transport>);

impl V1Binding {
    pub fn state(&self) -> ClientState {
        self.0.state()
    }

    pub async fn connect(&self) -> Result<()> {
        if self.state().status != ConnectionStatus::Connected {
            value::<PairResponse>(self.0.pair().await?)?;
        }
        Ok(())
    }

    pub async fn v1_snapshot(&self) -> Result<ManagerSnapshot> {
        self.v1_execute(&GetSnapshot {}).await
    }

    /// Sends one typed command against this binding. Mutations are never retried.
    pub async fn v1_execute<R: Request>(&self, request: &R) -> Result<R::Response> {
        value(self.v1_reply(request).await?)
    }

    pub(crate) async fn v1_reply<R: Request>(&self, request: &R) -> Result<Reply<R::Response>> {
        Ok(self.0.execute(request).await?)
    }

    pub async fn disconnect(&self) -> Result<()> {
        value::<Disconnected>(self.0.disconnect().await?)?;
        Ok(())
    }
}

fn value<T>(reply: Reply<T>) -> Result<T> {
    match reply {
        Reply::Success(value) => Ok(value),
        Reply::Failure(error) => Err(anyhow!(error.message)),
    }
}

impl super::HostClient {
    /// Creates an unpaired view. Keep it for subsequent calls to the same peer.
    pub fn peer_v1(&self, peer: AuthenticatedPeerId) -> V1Binding {
        V1Binding(Client::new(
            peer,
            ClientId::new_v4(),
            Transport(self.native.clone()),
        ))
    }
}
