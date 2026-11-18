//! Explicitly prepared V2 handlers. The network HTTP allowlist stays OFF.
use super::*;
use crate::{ObserverConnection, ObserverOperation, ObserverOwnerRequest};
use shellfox_protocol::{observer_v2 as wire, AuthenticatedPeerId, Endpoint};
use shellfox_transport::incoming::SignedHeaders;
#[path = "backend_observer_execute.rs"]
mod execute;
#[path = "backend_observer_pair.rs"]
mod pairing;
#[path = "backend_observer_state.rs"]
mod state;
use state::*;
#[cfg(test)]
#[path = "backend_observer_tests.rs"]
mod tests;

pub(super) struct ObserverBackend {
    owner_id: wire::Id,
    state: Arc<Mutex<Registry>>,
    mint_connection: fn() -> wire::Id,
}
impl Drop for ObserverBackend {
    fn drop(&mut self) {
        if let Ok(state) = self.state.lock() {
            for view in state.views.values() {
                view.connection.invalidate();
            }
        }
    }
}
/// Local adapter result, not a protocol envelope. Body is ONLY canonical V2 JSON.
pub struct ObserverHttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}
impl Backend {
    /// Builds real V2 handlers on the same owner channel, approvals and secret.
    /// This intentionally does NOT add a network route or change default clients.
    pub fn prepare_observer_v2(mut self) -> Self {
        self.observer_v2 = Some(ObserverBackend {
            owner_id: wire::Id::new(),
            state: Arc::default(),
            mint_connection: wire::Id::new,
        });
        self
    }
    pub fn observer_v2_owner_id(&self) -> Option<wire::Id> {
        self.observer_v2.as_ref().map(|v| v.owner_id)
    }
    /// Endpoint is closed; caller supplies the original signed bytes. No URL,
    /// HTTP-method or arbitrary-header forwarding is exposed by this function.
    pub async fn handle_observer_v2_signed(
        &self,
        endpoint: Endpoint,
        body: &[u8],
        headers: SignedHeaders<'_>,
    ) -> ObserverHttpReply {
        let Some(observer) = &self.observer_v2 else {
            return ObserverHttpReply {
                status: 404,
                body: Vec::new(),
            };
        };
        let mut binding = None;
        let result = async {
            if !matches!(
                endpoint,
                Endpoint::DiscoverV2
                    | Endpoint::PairV2
                    | Endpoint::RequestV2
                    | Endpoint::EventsV2
                    | Endpoint::DisconnectV2
            ) || body.len() > shellfox_protocol::MAX_REQUEST_BYTES
            {
                return Err(application(invalid()));
            }
            let now = (self.clock)();
            let peer = self
                .secret
                .verify_incoming(
                    SignedHeaders {
                        peer: headers.peer,
                        timestamp: headers.timestamp,
                        signature: headers.signature,
                    },
                    "POST",
                    endpoint.path(),
                    body,
                    now,
                )
                .ok_or_else(|| application(auth()))?;
            self.limit(
                &peer,
                headers.signature,
                headers.timestamp.parse().map_err(|_| application(auth()))?,
            )
            .map_err(application)?;
            match endpoint {
                Endpoint::DiscoverV2 => {
                    decode::<wire::Discover>(body)?;
                    checked(wire::Discovery {
                        service: wire::ServiceName::Shellfox,
                        owner_id: observer.owner_id,
                        profiles: vec![wire::PROFILE.into()],
                        catalog_modes: vec!["complete".into()],
                        delivery_formats: vec!["vt-bytes-base64-v1".into()],
                        recovery_formats: vec!["formatted-active-buffer-v1".into()],
                    })
                }
                Endpoint::PairV2 => self.observer_pair(observer, peer, decode(body)?, now).await,
                Endpoint::DisconnectV2 => {
                    self.observer_disconnect(observer, peer, decode(body)?, now)
                        .await
                }
                Endpoint::RequestV2 => {
                    let decoded = wire::decode_request(body).map_err(|_| application(invalid()))?;
                    let connection = self.observer_connection(observer, &peer, &decoded.binding)?;
                    let actual =
                        AuthenticatedPeerId::new(peer.id).map_err(|_| application(auth()))?;
                    decoded
                        .check_connection(connection.authenticated(), &actual)
                        .map_err(|_| application(auth()))?;
                    binding = Some(decoded.binding.clone());
                    decoded.command.authorize_profile()?;
                    crate::observer_policy::authorize(
                        connection.actor().permission,
                        &decoded.command,
                    )
                    .map_err(application)?;
                    self.observer_execute(observer, connection, Some(decoded.command))
                        .await
                }
                Endpoint::EventsV2 => {
                    let poll: wire::BoundPoll = decode(body)?;
                    let connection = self.observer_connection(observer, &peer, &poll.binding)?;
                    binding = Some(poll.binding);
                    self.observer_execute(observer, connection, None).await
                }
                _ => Err(application(invalid())),
            }
        }
        .await;
        encode_reply(binding, result)
    }
}
fn decode<T: serde::de::DeserializeOwned + Validate>(body: &[u8]) -> Result<T, wire::Error> {
    let value: T = serde_json::from_slice(body).map_err(|_| application(invalid()))?;
    value.validate().map_err(|_| application(invalid()))?;
    Ok(value)
}
fn checked<T: serde::Serialize + Validate>(value: T) -> Result<Value, wire::Error> {
    value.validate().map_err(|_| application(invalid()))?;
    serde_json::to_value(value).map_err(|_| application(invalid()))
}
fn application(error: AppError) -> wire::Error {
    wire::Error {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
        detail: wire::ErrorDetail::Application,
    }
}
fn encode_reply(
    binding: Option<wire::Binding>,
    result: Result<Value, wire::Error>,
) -> ObserverHttpReply {
    let outcome = match result {
        Ok(value) => wire::Reply::Success(value),
        Err(error) => wire::Reply::Failure(error),
    };
    let encoded = match binding.clone() {
        Some(binding) => serde_json::to_vec(&wire::BoundReply { binding, outcome }),
        None => serde_json::to_vec(&outcome),
    };
    let body = match encoded {
        Ok(bytes) if bytes.len() <= shellfox_protocol::MAX_RESPONSE_BYTES => bytes,
        _ => {
            let outcome = wire::Reply::<Value>::Failure(wire::Error {
                code: ErrorCode::Validation,
                message: "Observer response exceeds its bounded carrier.".into(),
                retryable: false,
                detail: wire::ErrorDetail::ResourceLimit {
                    limit: wire::Limit::ResponseBytes,
                },
            });
            match binding {
                Some(binding) => serde_json::to_vec(&wire::BoundReply { binding, outcome }),
                None => serde_json::to_vec(&outcome),
            }
            .unwrap_or_default()
        }
    };
    ObserverHttpReply { status: 200, body }
}
