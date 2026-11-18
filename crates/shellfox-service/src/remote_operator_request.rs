//! Deserialize directly into a closed wire shape. Empty State{} rejects extra
//! fields; a Serde unit variant would silently ignore them. No Value roundtrip.
use super::{AuthKey, RemoteRequest};
use uuid::Uuid;
#[derive(serde::Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Input {
    State {},
    Configure { enabled: bool, hostname: String },
    Login { auth_key: Option<AuthKey> },
    Cancel { attempt_id: Uuid },
    Continue { attempt_id: Uuid },
    Connect { peer_id: String },
    Disconnect { peer_id: String },
    Reconnect { peer_id: String },
    Approve { peer_id: String },
    Reject { peer_id: String },
    Revoke { peer_id: String },
    Forget { peer_id: String },
}
impl From<Input> for RemoteRequest {
    fn from(value: Input) -> Self {
        match value {
            Input::State {} => Self::State,
            Input::Configure { enabled, hostname } => Self::Configure { enabled, hostname },
            Input::Login { auth_key } => Self::Login { auth_key },
            Input::Cancel { attempt_id } => Self::Cancel { attempt_id },
            Input::Continue { attempt_id } => Self::Continue { attempt_id },
            Input::Connect { peer_id } => Self::Connect { peer_id },
            Input::Disconnect { peer_id } => Self::Disconnect { peer_id },
            Input::Reconnect { peer_id } => Self::Reconnect { peer_id },
            Input::Approve { peer_id } => Self::Approve { peer_id },
            Input::Reject { peer_id } => Self::Reject { peer_id },
            Input::Revoke { peer_id } => Self::Revoke { peer_id },
            Input::Forget { peer_id } => Self::Forget { peer_id },
        }
    }
}
impl<'de> serde::Deserialize<'de> for RemoteRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <Input as serde::Deserialize>::deserialize(deserializer).map(Into::into)
    }
}
