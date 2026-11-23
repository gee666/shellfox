//! `--host <peer-id|hostname>`: one v1 view of an approved peer, over the local
//! host's own tailnet node, as 0.9 forwarded `remote.request`. Only the v1 remote
//! allowlist crosses; the peer applies its own approval and permission policy.
use crate::{manager::Call, output::CliError};
use anyhow::Result;
use serde_json::Value;
use shellfox_client::Client;
use shellfox_protocol::{
    AuthenticatedPeerId, ClientId, Endpoint, OutboundTransport, Reply, RequestLimits,
    TransportError, TransportFuture,
};
use shellfox_service::{
    local::LocalClient,
    remote::{OutgoingV2Endpoint, RemoteRequest},
};

struct Outgoing<'a>(&'a LocalClient);
impl OutboundTransport for Outgoing<'_> {
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

/// Any v1 transport: the local host's tailnet node, or a test peer.
struct Transport<'a>(Box<dyn OutboundTransport + 'a>);
impl OutboundTransport for Transport<'_> {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        self.0.send(peer, endpoint, body, limits)
    }
}

pub struct Peer<'a> {
    client: Client<Transport<'a>>,
}

fn uncertain() -> anyhow::Error {
    CliError::new(
        "TARGET_LOST",
        "The remote host did not answer. The outcome is unknown and the request was not retried.",
        true,
    )
}
fn value(reply: Reply<Value>) -> Result<Value> {
    match reply {
        Reply::Success(value) => Ok(value),
        Reply::Failure(error) => Err(CliError::app(error)),
    }
}

/// 0.9 `resolveTarget`: an exact peer ID, else one case-insensitive hostname.
pub async fn resolve(local: &LocalClient, target: &str) -> Result<AuthenticatedPeerId> {
    let state = local.remote_control(RemoteRequest::State).await?;
    let id = match state.peers.iter().find(|peer| peer.id == target) {
        Some(peer) => peer.id.clone(),
        None => {
            let matches: Vec<_> = state
                .peers
                .iter()
                .filter(|peer| peer.hostname.eq_ignore_ascii_case(target))
                .collect();
            match matches.as_slice() {
                [peer] => peer.id.clone(),
                [] => return Err(CliError::usage("Unknown host. Run shellfox machines list.")),
                _ => return Err(CliError::usage("Host name is ambiguous. Use a peer ID.")),
            }
        }
    };
    AuthenticatedPeerId::new(id).map_err(|_| CliError::usage("Invalid peer ID."))
}

impl<'a> Peer<'a> {
    /// Pairs a fresh view. The peer must already have approved this machine.
    pub async fn connect(local: &'a LocalClient, peer: AuthenticatedPeerId) -> Result<Self> {
        Self::pair(peer, Box::new(Outgoing(local))).await
    }
    async fn pair(
        peer: AuthenticatedPeerId,
        transport: Box<dyn OutboundTransport + 'a>,
    ) -> Result<Self> {
        let client = Client::new(peer, ClientId::new_v4(), Transport(transport));
        match client.pair().await.map_err(|_| uncertain())? {
            Reply::Success(_) => Ok(Self { client }),
            Reply::Failure(error) => Err(CliError::app(error)),
        }
    }
    pub async fn call(&self, call: Call) -> Result<Value> {
        let Call::Remote(command) = call else {
            return Err(CliError::new(
                "UNSUPPORTED",
                format!("{} is not available over a remote connection.", call.name()),
                false,
            ));
        };
        value(
            self.client
                .execute_command(&command)
                .await
                .map_err(|_| uncertain())?,
        )
    }
    pub async fn close(self) {
        let _ = self.client.disconnect().await;
    }
}

#[cfg(test)]
#[path = "manager_peer_tests.rs"]
mod tests;
