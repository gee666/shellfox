use crate::HelperError;
/// No diagnostics expose secret contents.
#[derive(Clone)]
pub struct Secret(String);
impl Secret {
    pub fn new(value: String) -> Result<Self, HelperError> {
        if !(32..=256).contains(&value.len()) || !value.bytes().all(|b| (33..=126).contains(&b)) {
            return Err(HelperError::Configuration);
        }
        Ok(Self(value))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl Secret {
    /// Authenticate only the helper's incoming identity. The host must still
    /// enforce approval, revocation and per-view permissions before dispatch.
    pub fn verify_incoming(
        &self,
        headers: crate::incoming::SignedHeaders<'_>,
        method: &str,
        path: &str,
        body: &[u8],
        now: u64,
    ) -> Option<crate::incoming::PeerIdentity> {
        crate::incoming::verify_peer(self.expose().as_bytes(), headers, method, path, body, now)
    }
}
