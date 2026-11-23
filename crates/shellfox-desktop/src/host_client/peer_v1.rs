//! A v1 view bound to one authenticated tailnet peer through the existing Go helper.
use anyhow::{Result, anyhow};
use shellfox_client::{Client, ClientState, ConnectionStatus};
use shellfox_protocol::{
    AuthenticatedPeerId, ClientId, Disconnected, Endpoint, GetSnapshot, ManagerSnapshot,
    OutboundTransport, PairResponse, PollResponse, Reply, Request, RequestLimits, TransportError,
    TransportFuture,
};
use std::sync::Arc;

#[cfg(test)]
mod interop_tests;

struct Transport(super::attached::AttachedLocal);

impl OutboundTransport for Transport {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let response = self
                .0
                .outgoing(peer.clone(), endpoint, body, limits)
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

pub struct V1Binding {
    client: Client<Transport>,
    pairing: tokio::sync::Mutex<()>,
    /// Bumped by the client after each status, permission or connection change.
    changes: Arc<tokio::sync::watch::Sender<u64>>,
}

impl V1Binding {
    pub fn state(&self) -> ClientState {
        self.client.state()
    }

    /// Each view of this peer observes the same state changes.
    pub(crate) fn state_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
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
        let mut reply = self
            .v1_reply_with_connection(connection, &GetSnapshot {})
            .await?;
        // A publication or exit can invalidate an in-flight catalog read.
        // Refresh once, only on the same view. Mutations are never retried.
        if matches!(&reply, Reply::Failure(error) if error.code == shellfox_protocol::ErrorCode::TargetLost)
        {
            reply = self
                .v1_reply_with_connection(connection, &GetSnapshot {})
                .await?;
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
        let changes = Arc::new(tokio::sync::watch::channel(0u64).0);
        let client = Client::new(peer, ClientId::new_v4(), Transport(self.port.clone()));
        let wake = changes.clone();
        client.set_state_waker(Some(Arc::new(move || {
            wake.send_modify(|count| *count = count.wrapping_add(1));
        })));
        V1Binding {
            client,
            pairing: tokio::sync::Mutex::new(()),
            changes,
        }
    }
}
