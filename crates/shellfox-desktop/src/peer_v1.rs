//! A v1 view bound to one authenticated tailnet peer through the existing Go helper.
use anyhow::{anyhow, Result};
use shellfox_client::{Client, ClientState, ConnectionStatus};
use shellfox_protocol::{
    AuthenticatedPeerId, ClientId, Disconnected, Endpoint, GetSnapshot, ManagerSnapshot,
    OutboundTransport, PairResponse, PollResponse, Reply, Request, RequestLimits, TransportError,
    TransportFuture,
};
use shellfox_service::{native::NativeClient, remote::OutgoingV2Endpoint};

#[cfg(test)]
#[path = "peer_v1_interop_tests.rs"]
mod interop_tests;

enum Transport {
    Native(NativeClient),
    Attached(super::attached::AttachedLocal),
}

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
            let response = match self {
                Self::Native(client) => {
                    client
                        .outgoing_v2_with_status(peer, endpoint, body, limits)
                        .await?
                }
                Self::Attached(client) => {
                    client
                        .outgoing(peer.clone(), endpoint, body, limits)
                        .await?
                }
            };
            if !(200..300).contains(&response.status) {
                return Err(TransportError::Rejected {
                    status: response.status,
                });
            }
            Ok(response.body)
        })
    }
}

pub struct V1Binding {
    client: Client<Transport>,
    pairing: tokio::sync::Mutex<()>,
}

impl V1Binding {
    pub fn state(&self) -> ClientState {
        self.client.state()
    }

    pub async fn connect(&self) -> Result<()> {
        let _pairing = self.pairing.lock().await;
        let state = self.state();
        match state.status {
            ConnectionStatus::Connected => Ok(()),
            ConnectionStatus::Rejected => Err(anyhow!(state.last_error.map_or_else(
                || "Access declined or revoked.".into(),
                |error| error.message,
            ))),
            _ => value::<PairResponse>(self.client.pair().await?).map(|_| ()),
        }
    }

    pub(super) fn retry_rejected(&self) {
        if self.state().status == ConnectionStatus::Rejected {
            self.client.reconnect(ClientId::new_v4());
        }
    }

    pub(crate) async fn poll(&self) -> Result<PollResponse> {
        value(self.client.poll().await?)
    }

    pub async fn v1_snapshot(&self) -> Result<ManagerSnapshot> {
        let connection = self.state().connection_id;
        let mut reply = self.v1_reply_with_connection(connection, &GetSnapshot {}).await?;
        // A publication or exit can invalidate an in-flight catalog read.
        // Refresh once, only on the same view. Mutations are never retried.
        if matches!(&reply, Reply::Failure(error) if error.code == shellfox_protocol::ErrorCode::TargetLost) {
            reply = self.v1_reply_with_connection(connection, &GetSnapshot {}).await?;
        }
        value(reply)
    }

    /// Sends one typed command against this binding. Mutations are never retried.
    pub async fn v1_execute<R: Request>(&self, request: &R) -> Result<R::Response> {
        value(self.v1_reply(request).await?)
    }

    pub(crate) async fn v1_reply<R: Request>(&self, request: &R) -> Result<Reply<R::Response>> {
        Ok(self.client.execute(request).await?)
    }

    pub(crate) async fn v1_reply_with_connection<R: Request>(
        &self,
        connection: u64,
        request: &R,
    ) -> Result<Reply<R::Response>> {
        Ok(self
            .client
            .execute_with_connection(connection, request)
            .await?)
    }

    pub(crate) fn with_connection<T>(
        &self,
        connection: u64,
        operation: impl FnOnce() -> T,
    ) -> Result<T> {
        Ok(self.client.with_connection(connection, operation)?)
    }

    pub async fn disconnect(&self) -> Result<()> {
        let _pairing = self.pairing.lock().await;
        value::<Disconnected>(self.client.disconnect().await?)?;
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
        V1Binding {
            client: Client::new(
                peer,
                ClientId::new_v4(),
                match &self.port {
                    super::HostPort::Native(client) => Transport::Native(client.clone()),
                    super::HostPort::Attached(client) => Transport::Attached(client.clone()),
                },
            ),
            pairing: tokio::sync::Mutex::new(()),
        }
    }
}
