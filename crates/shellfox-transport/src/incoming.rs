//! Verify the Go helper's signed incoming contract. The host must separately
//! apply device approval, per-view permissions and revocation on every operation.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerIdentity {
    pub id: String,
    pub hostname: String,
}

/// Pass exactly one value for each header. HTTP adapters must reject duplicates.
/// Method, raw URL path and body must be the original bytes, never re-encoded JSON.
pub struct SignedHeaders<'a> {
    pub peer: &'a str,
    pub timestamp: &'a str,
    pub signature: &'a str,
}

pub fn verify_peer(
    secret: &[u8],
    headers: SignedHeaders<'_>,
    method: &str,
    raw_path: &str,
    body: &[u8],
    now_millis: u64,
) -> Option<PeerIdentity> {
    if headers.peer.len() > 2048
        || headers.timestamp.len() != 13
        || !headers.timestamp.bytes().all(|b| b.is_ascii_digit())
        || headers.signature.len() != 64
        || !headers
            .signature
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let timestamp: u64 = headers.timestamp.parse().ok()?;
    if now_millis.abs_diff(timestamp) > 30_000 {
        return None;
    }
    let signature = hex::decode(headers.signature).ok()?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    update_message(
        &mut mac,
        headers.timestamp,
        headers.peer,
        method,
        raw_path,
        body,
    );
    mac.verify_slice(&signature).ok()?;
    let identity: PeerIdentity =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(headers.peer).ok()?).ok()?;
    crate::AuthenticatedPeerId::new(&identity.id).ok()?;
    if identity.hostname.is_empty()
        || identity.hostname.len() > 253
        || identity.hostname.chars().any(char::is_control)
    {
        return None;
    }
    Some(identity)
}

fn update_message(
    mac: &mut Hmac<Sha256>,
    timestamp: &str,
    peer: &str,
    method: &str,
    path: &str,
    body: &[u8],
) {
    for value in [timestamp, peer, method, path] {
        mac.update(value.as_bytes());
        mac.update(b"\n");
    }
    mac.update(hex::encode(Sha256::digest(body)).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    const SIGNATURE: &str = "de4dad52bcc645ae868d2e8975fdb3bae5cd378523e0ea604bf48f87681dc74f";
    fn verify(body: &[u8], method: &str, path: &str, now: u64) -> Option<PeerIdentity> {
        let peer = URL_SAFE_NO_PAD.encode(br#"{"id":"n-peer","hostname":"my laptop"}"#);
        verify_peer(
            &[b'b'; 32],
            SignedHeaders {
                peer: &peer,
                timestamp: "1700000000123",
                signature: SIGNATURE,
            },
            method,
            path,
            body,
            now,
        )
    }
    #[test]
    fn existing_go_and_node_hmac_vector() {
        // Use the actual pinned contract vector, not a Rust-generated expected digest.
        assert!(include_str!("../../../native/tailscale/bridge_test.go").contains(SIGNATURE));
        let secret = crate::Secret::new("b".repeat(32)).unwrap();
        let peer = URL_SAFE_NO_PAD.encode(br#"{"id":"n-peer","hostname":"my laptop"}"#);
        assert!(secret
            .verify_incoming(
                SignedHeaders {
                    peer: &peer,
                    timestamp: "1700000000123",
                    signature: SIGNATURE
                },
                "POST",
                "/v1/request",
                br#"{"hello":"world"}"#,
                1700000000123
            )
            .is_some());
        assert_eq!(format!("{secret:?}"), "Secret([redacted])");
        assert_eq!(
            verify(
                br#"{"hello":"world"}"#,
                "POST",
                "/v1/request",
                1700000000123
            )
            .unwrap()
            .id,
            "n-peer"
        );
        assert!(verify(
            br#"{"hello":"world"} "#,
            "POST",
            "/v1/request",
            1700000000123
        )
        .is_none());
        assert!(verify(br#"{"hello":"world"}"#, "GET", "/v1/request", 1700000000123).is_none());
        assert!(verify(br#"{"hello":"world"}"#, "POST", "/v1/pair", 1700000000123).is_none());
        assert!(verify(
            br#"{"hello":"world"}"#,
            "POST",
            "/v1/request",
            1700000030124
        )
        .is_none());
        assert!(verify(
            br#"{"hello":"world"}"#,
            "POST",
            "/v1/request",
            1699999970122
        )
        .is_none());
        assert!(verify(
            br#"{"hello":"world"}"#,
            "POST",
            "/v1/request",
            1700000030123
        )
        .is_some());
    }
}
