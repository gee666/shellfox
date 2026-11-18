use crate::HelperError;
use serde::Deserialize;
use serde_json::Value;
use std::net::IpAddr;

pub const LINE_LIMIT: usize = 1024 * 1024;
pub const COMMAND_LIMIT: usize = 64 * 1024 - 1;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Probe {
    pub protocol_version: u32,
    pub tailscale_version: String,
    pub platform: String,
    pub arch: String,
}
impl Probe {
    pub(crate) fn validate(&self) -> Result<(), HelperError> {
        if self.protocol_version != 1
            || self.tailscale_version.is_empty()
            || self.tailscale_version.len() > 64
            || !["windows", "linux", "darwin"].contains(&self.platform.as_str())
            || !["amd64", "arm64", "386", "arm"].contains(&self.arch.as_str())
        {
            return Err(HelperError::Protocol);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkState {
    Disabled,
    Starting,
    LoginRequired,
    Online,
    Error,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkStatus {
    pub status: NetworkState,
    #[serde(deserialize_with = "nullable_string")]
    pub login_url: Option<String>,
    #[serde(deserialize_with = "nullable_string")]
    pub error: Option<String>,
}
fn nullable_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
impl NetworkStatus {
    pub(crate) fn validate(mut self) -> Result<Self, HelperError> {
        if self.error.as_ref().is_some_and(|v| v.len() > 1000) {
            return Err(HelperError::Protocol);
        }
        // Do not propagate helper diagnostics, even from an otherwise valid status event.
        self.error = self.error.map(|_| "Tailscale network unavailable.".into());
        if let Some(url) = &self.login_url {
            if url.len() > 2048
                || !url.starts_with("https://login.tailscale.com/")
                || url.chars().any(|c| c.is_control() || c.is_whitespace())
            {
                return Err(HelperError::Protocol);
            }
        }
        if self.status != NetworkState::LoginRequired {
            self.login_url = None;
        }
        Ok(self)
    }
}
impl std::fmt::Debug for NetworkStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkStatus")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPeer {
    pub id: String,
    pub hostname: String,
    pub addresses: Vec<String>,
    pub online: bool,
}
impl NetworkPeer {
    pub(crate) fn validate(&self) -> Result<(), HelperError> {
        crate::PeerId::new(&self.id).map_err(|_| HelperError::Protocol)?;
        if !valid_label(&self.hostname)
            || self.addresses.is_empty()
            || self.addresses.len() > 16
            || !self
                .addresses
                .iter()
                .all(|v| v.parse::<IpAddr>().is_ok_and(tailnet_address))
        {
            return Err(HelperError::Protocol);
        }
        Ok(())
    }
}
pub(crate) fn valid_label(value: &str) -> bool {
    !value.is_empty() && value.len() <= 253 && !value.chars().any(char::is_control)
}
fn tailnet_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let b = ip.octets();
            b[0] == 100 && (64..=127).contains(&b[1])
        }
        IpAddr::V6(ip) => {
            let b = ip.segments();
            b[0] == 0xfd7a && b[1] == 0x115c && b[2] == 0xa1e0
        }
    }
}

pub(crate) enum Frame {
    Status(NetworkStatus),
    Reply {
        id: String,
        value: Result<Value, HelperError>,
    },
}
#[derive(Deserialize)]
#[serde(untagged)]
enum WireFrame {
    Event(Event),
    Reply(Reply),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    event: String,
    payload: NetworkStatus,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Reply {
    Success(Success),
    Failure(Failure),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success {
    id: String,
    ok: bool,
    value: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    id: String,
    ok: bool,
    error: WireError,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireError {
    code: String,
    message: String,
}

pub(crate) fn decode(line: &[u8]) -> Result<Frame, HelperError> {
    if line.len() > LINE_LIMIT {
        return Err(HelperError::Protocol);
    }
    match serde_json::from_slice(line).map_err(|_| HelperError::Protocol)? {
        WireFrame::Event(e) if e.event == "status" => Ok(Frame::Status(e.payload.validate()?)),
        WireFrame::Reply(Reply::Success(r)) if r.ok && valid_id(&r.id) => Ok(Frame::Reply {
            id: r.id,
            value: Ok(r.value),
        }),
        WireFrame::Reply(Reply::Failure(r))
            if !r.ok
                && valid_id(&r.id)
                && r.error.code.len() <= 64
                && r.error.message.len() <= 4096 =>
        {
            Ok(Frame::Reply {
                id: r.id,
                value: Err(HelperError::Command),
            })
        }
        _ => Err(HelperError::Protocol),
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoder_rejects_ambiguous_unknown_or_unbounded_protocol_data() {
        for line in [
            r#"{"id":"1","ok":true,"value":{},"extra":true}"#,
            r#"{"id":"1","id":"2","ok":true,"value":{}}"#,
            r#"{"id":1,"ok":true,"value":{}}"#,
            r#"{"id":"1","ok":false,"value":{}}"#,
            r#"{"event":"status","payload":{"status":"online","loginUrl":null}}"#,
            r#"{"event":"unknown","payload":{"status":"online","loginUrl":null,"error":null}}"#,
            r#"{"event":"status","payload":{"status":"login-required","loginUrl":"https://login.tailscale.com.evil.invalid/a","error":null}}"#,
        ] {
            assert!(
                matches!(decode(line.as_bytes()), Err(HelperError::Protocol)),
                "{line}"
            );
        }
        assert!(decode(&vec![b'x'; LINE_LIMIT + 1]).is_err());
    }
    #[test]
    fn status_diagnostics_and_inactive_login_urls_do_not_escape() {
        let frame=decode(br#"{"event":"status","payload":{"status":"error","loginUrl":"https://login.tailscale.com/a/secret","error":"tskey-auth-sensitive"}}"#).unwrap();
        let Frame::Status(status) = frame else {
            panic!("expected event")
        };
        assert!(status.login_url.is_none());
        assert_eq!(
            status.error.as_deref(),
            Some("Tailscale network unavailable.")
        );
        assert!(!format!("{status:?}").contains("secret"));
    }
    #[test]
    fn peers_only_accept_valid_ids_and_tailnet_addresses() {
        for address in ["100.64.0.1", "100.127.255.254", "fd7a:115c:a1e0::2"] {
            assert!(tailnet_address(address.parse().unwrap()));
        }
        for address in [
            "127.0.0.1",
            "100.128.0.1",
            "192.168.1.1",
            "::ffff:100.64.0.1",
            "::1",
        ] {
            assert!(!tailnet_address(address.parse().unwrap()));
        }
    }
}
