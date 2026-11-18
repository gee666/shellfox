//! Explicitly prepared manager handlers. The network HTTP allowlist stays OFF.
use super::observer::ObserverHttpReply;
/// Local HTTP carrier, not a protocol envelope. Body is canonical manager-v2 JSON.
pub type ManagerHttpReply = ObserverHttpReply;
use super::*;
use crate::{ManagerConnection, ManagerOperation, ManagerOwnerRequest};
use shellfox_protocol::{manager_v2 as wire, AuthenticatedPeerId, Endpoint};
use shellfox_transport::incoming::SignedHeaders;
#[path = "backend_manager_execute.rs"]
mod execute;
#[path = "backend_manager_pair.rs"]
mod pairing;
#[path = "backend_manager_state.rs"]
mod state;
use state::*;
#[cfg(test)]
#[path = "backend_manager_tests.rs"]
mod tests;

pub(super) struct ManagerBackend {
    owner_id: wire::Id,
    state: Arc<Mutex<Registry>>,
    mint_connection: fn() -> wire::Id,
    native0_sender: Option<wire::Native0Codec>,
}
impl Drop for ManagerBackend {
    fn drop(&mut self) {
        if let Ok(state) = self.state.lock() {
            for view in state.views.values() {
                view.connection.invalidate();
            }
        }
    }
}
impl Backend {
    /// Builds real V2 handlers on the same owner channel, approvals and secret.
    /// This intentionally does NOT add a network route or change default clients.
    pub fn prepare_manager_v2(mut self) -> Self {
        self.manager_v2 = Some(ManagerBackend {
            owner_id: wire::Id::new(),
            state: Arc::default(),
            mint_connection: wire::Id::new,
            native0_sender: None,
        });
        self
    }
    /// Trusted Rust composition only: sender MUST come from the actual local
    /// verified Runtime engine. Never accept this capability from request JSON.
    /// Source recovery checks the captured engine against this selection again.
    pub fn prepare_manager_v2_native0(self, sender: wire::Native0Codec) -> Result<Self, AppError> {
        sender.validate().map_err(|_| invalid())?;
        let mut backend = self.prepare_manager_v2();
        if let Some(manager) = &mut backend.manager_v2 {
            manager.native0_sender = Some(sender);
        }
        Ok(backend)
    }
    pub fn manager_v2_owner_id(&self) -> Option<wire::Id> {
        self.manager_v2.as_ref().map(|v| v.owner_id)
    }
    /// Endpoint is closed; caller supplies the original signed bytes. No URL,
    /// HTTP-method or arbitrary-header forwarding is exposed by this function.
    pub async fn handle_manager_v2_signed(
        &self,
        endpoint: Endpoint,
        body: &[u8],
        headers: SignedHeaders<'_>,
    ) -> ObserverHttpReply {
        let Some(manager) = &self.manager_v2 else {
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
                        service: shellfox_protocol::observer_v2::ServiceName::Shellfox,
                        owner_id: manager.owner_id,
                        profiles: vec![wire::PROFILE.into()],
                        catalog_modes: vec!["complete".into()],
                        delivery_formats: vec!["vt-bytes-base64-v1".into()],
                        recovery_formats: if manager.native0_sender.is_some() {
                            vec!["formatted-active-buffer-v1".into(), "native0-raw-v1".into()]
                        } else {
                            vec!["formatted-active-buffer-v1".into()]
                        },
                    })
                }
                Endpoint::PairV2 => self.manager_pair(manager, peer, decode(body)?, now).await,
                Endpoint::DisconnectV2 => {
                    self.manager_disconnect(manager, peer, decode(body)?, now)
                        .await
                }
                Endpoint::RequestV2 => {
                    let decoded = wire::decode_request(body).map_err(|_| application(invalid()))?;
                    let connection = self.manager_connection(manager, &peer, &decoded.binding)?;
                    let actual =
                        AuthenticatedPeerId::new(peer.id).map_err(|_| application(auth()))?;
                    decoded
                        .check_connection(connection.authenticated(), &actual)
                        .map_err(|_| application(auth()))?;
                    binding = Some(decoded.binding.clone());
                    crate::manager_policy::authorize(connection.actor(), &decoded.command)
                        .map_err(application)?;
                    self.manager_execute(manager, connection, Some(decoded.command))
                        .await
                }
                Endpoint::EventsV2 => {
                    let poll: wire::BoundPoll = decode(body)?;
                    let connection = self.manager_connection(manager, &peer, &poll.binding)?;
                    binding = Some(poll.binding);
                    self.manager_execute(manager, connection, None).await
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
fn input_unknown(input_id: wire::Id, message: &str) -> wire::Error {
    wire::Error {
        code: ErrorCode::NativeUnavailable,
        message: message.into(),
        retryable: false,
        detail: wire::ErrorDetail::InputOutcomeUnknown {
            input_id,
            retry_input: false,
        },
    }
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
                message: "Manager response exceeds its bounded carrier.".into(),
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
