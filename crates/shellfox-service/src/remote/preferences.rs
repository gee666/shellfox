//! Private remote UI preferences. No keys, output, environments or login URLs.
use super::{native::*, RemoteBackend};
use serde::{Deserialize, Serialize};
use shellfox_protocol::AppError;
pub(in crate::remote) mod store;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RememberedPeer {
    pub id: String,
    pub hostname: String,
    pub addresses: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemotePreferences {
    pub version: u8,
    pub enabled: bool,
    pub hostname: String,
    pub peers: Vec<RememberedPeer>,
}
impl RemotePreferences {
    pub fn validate(&self) -> Result<(), AppError> {
        RemoteRequest::Configure {
            enabled: self.enabled,
            hostname: self.hostname.clone(),
        }
        .validate()?;
        if self.version != 1 || self.peers.len() > 1000 {
            return Err(failed());
        }
        let mut ids = std::collections::HashSet::new();
        for peer in &self.peers {
            RemoteRequest::Connect {
                peer_id: peer.id.clone(),
            }
            .validate()?;
            if !ids.insert(&peer.id)
                || peer.hostname.is_empty()
                || peer.hostname.len() > 253
                || peer.hostname.chars().any(char::is_control)
                || peer.addresses.len() > 16
                || peer
                    .addresses
                    .iter()
                    .any(|address| address.parse::<std::net::IpAddr>().is_err())
            {
                return Err(failed());
            }
        }
        Ok(())
    }
}
fn failed() -> AppError {
    error(
        shellfox_protocol::ErrorCode::StorageFailed,
        "Private remote preferences unavailable.",
    )
}
impl RemoteBackend {
    pub(super) async fn read_preferences(&self) -> Result<Option<RemotePreferences>, AppError> {
        self.preferences.read()
    }
    pub(super) async fn write_preferences(
        &self,
        preferences: RemotePreferences,
    ) -> Result<(), AppError> {
        // Visible publication only; this is not a durable approval commit.
        self.preferences.write(preferences)
    }
}
