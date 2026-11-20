//! Host-only remote settings contracts. Never accepted by the network allowlist.
use shellfox_protocol::{AppError, AuthenticatedPeerId, ErrorCode};
pub use shellfox_remote::ViewPermission;
use std::fmt;
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;
#[path = "remote_operator_request.rs"]
mod operator_request;

#[derive(Clone, serde::Serialize)]
#[serde(transparent)]
pub struct AuthKey(String);
impl<'de> serde::Deserialize<'de> for AuthKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(|_| serde::de::Error::custom("Invalid authentication key"))
    }
}
impl AuthKey {
    pub fn new(value: String) -> Result<Self, AppError> {
        if value.len() > 4096
            || !value.strip_prefix("tskey-auth-").is_some_and(|rest| {
                !rest.is_empty()
                    && rest
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        {
            return Err(invalid());
        }
        Ok(Self(value))
    }
    pub(super) fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for AuthKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthKey([redacted])")
    }
}
#[derive(Clone, Debug, serde::Serialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteRequest {
    State,
    Configure {
        enabled: bool,
        hostname: String,
    },
    Login {
        auth_key: Option<AuthKey>,
    },
    Cancel {
        attempt_id: Uuid,
    },
    Continue {
        attempt_id: Uuid,
    },
    #[serde(skip)]
    ContinueDesktopPair {
        attempt_id: Uuid,
        permit: PairingPermit,
    },
    Connect {
        peer_id: String,
    },
    Disconnect {
        peer_id: String,
    },
    Reconnect {
        peer_id: String,
    },
    Approve {
        peer_id: String,
    },
    Reject {
        peer_id: String,
    },
    Revoke {
        peer_id: String,
    },
    Forget {
        peer_id: String,
    },
}
impl RemoteRequest {
    pub fn validate(&self) -> Result<(), AppError> {
        match self {
            Self::Configure { hostname, .. } => {
                if hostname.is_empty()
                    || hostname.len() > 63
                    || !hostname.as_bytes()[0].is_ascii_alphanumeric()
                    || !hostname.as_bytes()[hostname.len() - 1].is_ascii_alphanumeric()
                    || !hostname
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err(invalid());
                }
            }
            Self::Cancel { attempt_id }
            | Self::Continue { attempt_id }
            | Self::ContinueDesktopPair { attempt_id, .. }
                if attempt_id.is_nil() =>
            {
                return Err(invalid())
            }
            Self::Connect { peer_id }
            | Self::Disconnect { peer_id }
            | Self::Reconnect { peer_id }
            | Self::Reject { peer_id }
            | Self::Revoke { peer_id }
            | Self::Forget { peer_id }
            | Self::Approve { peer_id, .. } => {
                AuthenticatedPeerId::new(peer_id.clone()).map_err(|_| invalid())?;
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteNetworkStatus {
    Disabled,
    Starting,
    LoginRequired,
    Online,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteConnection {
    Disconnected,
    Connecting,
    Reconnecting,
    AwaitingApproval,
    Rejected,
    Connected,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemotePeer {
    pub id: String,
    pub hostname: String,
    pub addresses: Vec<String>,
    pub online: bool,
    pub remembered: bool,
    pub host_confirmed: bool,
    pub connection: RemoteConnection,
    pub last_error: Option<AppError>,
    pub attempt_id: Option<Uuid>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AttemptKind {
    Resume,
    Enable,
    Login,
    Connect { peer_id: String },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAttempt {
    pub id: Uuid,
    pub kind: AttemptKind,
}
#[derive(Clone)]
pub struct RemoteControlState {
    pub enabled: bool,
    pub hostname: String,
    pub status: RemoteNetworkStatus,
    pub login_url: Option<String>,
    pub error: Option<AppError>,
    pub peers: Vec<RemotePeer>,
    pub pending: Vec<shellfox_remote::PendingPeer>,
    pub approvals: Vec<shellfox_remote::Approval>,
    pub attempts: Vec<RemoteAttempt>,
    /// Actual Rust-host restriction, not a claim about a connected Node host.
    pub cleanup_retained: bool,
    pub incoming_legacy_terminal_attachment: bool,
    pub terminal_capability_reason: String,
}
impl fmt::Debug for RemoteControlState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteControlState")
            .field("enabled", &self.enabled)
            .field("status", &self.status)
            .field("peers", &self.peers.len())
            .finish_non_exhaustive()
    }
}
pub type RemoteResponse = RemoteControlState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectResult {
    Created(Uuid),
    Existing,
}

#[derive(Clone, Debug)]
pub struct PairingPermit {
    pub deadline: std::time::Instant,
    pub lifetime: shellfox_protocol::CancellationToken,
}
impl PairingPermit {
    pub fn active_at(&self, now: std::time::Instant) -> bool {
        now < self.deadline && !self.lifetime.is_cancelled()
    }
}

#[derive(Debug)]
pub struct RemoteReceipt {
    pub state: RemoteResponse,
    pub connect: Option<ConnectResult>,
}
pub(super) struct RemoteEnvelope {
    pub request: RemoteRequest,
    pub reply: oneshot::Sender<Result<RemoteReceipt, AppError>>,
}
#[derive(Clone)]
pub struct RemoteControl {
    pub(super) sender: mpsc::Sender<RemoteEnvelope>,
    pub(super) outgoing: mpsc::Sender<super::outgoing::OutgoingEnvelope>,
    pub(super) state: watch::Receiver<RemoteControlState>,
    pub(super) runtime: tokio::runtime::Handle,
}
pub enum RemoteAdmissionError {
    Full(RemoteRequest),
    Stopped(RemoteRequest),
    Invalid(AppError),
}
pub type RemoteAccepted = oneshot::Receiver<Result<RemoteReceipt, AppError>>;
impl fmt::Debug for RemoteAdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "RemoteAdmissionError::Full",
            Self::Stopped(_) => "RemoteAdmissionError::Stopped",
            Self::Invalid(_) => "RemoteAdmissionError::Invalid",
        })
    }
}
impl RemoteControl {
    pub fn try_submit(
        &self,
        request: RemoteRequest,
    ) -> Result<RemoteAccepted, RemoteAdmissionError> {
        request.validate().map_err(RemoteAdmissionError::Invalid)?;
        let (reply, receive) = oneshot::channel();
        self.sender
            .try_send(RemoteEnvelope { request, reply })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(envelope) => {
                    RemoteAdmissionError::Full(envelope.request)
                }
                mpsc::error::TrySendError::Closed(envelope) => {
                    RemoteAdmissionError::Stopped(envelope.request)
                }
            })?;
        Ok(receive)
    }
    pub fn snapshot(&self) -> RemoteControlState {
        self.state.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<RemoteControlState> {
        self.state.clone()
    }
    pub async fn request(&self, request: RemoteRequest) -> Result<RemoteResponse, AppError> {
        let receive = self
            .try_submit(request)
            .map_err(|admission| match admission {
                RemoteAdmissionError::Invalid(error) => error,
                _ => error(
                    ErrorCode::NativeUnavailable,
                    "Remote control queue full or stopped. Request was not accepted.",
                ),
            })?;
        let receipt = receive.await.map_err(|_| {
            error(
                ErrorCode::NativeUnavailable,
                "Remote control stopped. Do not automatically retry an accepted action.",
            )
        })??;
        Ok(receipt.state)
    }
}
pub(super) fn error(code: ErrorCode, message: &str) -> AppError {
    AppError {
        code,
        message: message.into(),
        retryable: false,
    }
}
fn invalid() -> AppError {
    error(
        ErrorCode::Validation,
        "Invalid host remote settings request.",
    )
}
