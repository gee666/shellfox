//! Strict private transport frames, not application/remote/native UI methods.
use super::*;
pub(super) const TRANSPORT_VERSION: u8 = 1;
/// The only byte a host writes on a lifetime after opening it: an orderly stop.
pub(super) const LIFETIME_STOPPING: u8 = b's';
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Frame {
    LifetimeOpen {
        version: u8,
        secret: String,
        client_id: uuid::Uuid,
    },
    Request {
        version: u8,
        secret: String,
        client_id: uuid::Uuid,
        epoch: uuid::Uuid,
        body: Vec<u8>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub(super) enum LifetimeReply {
    #[serde(rename = "lifetimeOpened")]
    Opened {
        version: u8,
        client_id: uuid::Uuid,
        epoch: uuid::Uuid,
    },
}
impl Frame {
    pub fn authenticate(&self, expected: &str) -> Result<()> {
        let (version, secret, client) = match self {
            Self::LifetimeOpen {
                version,
                secret,
                client_id,
            }
            | Self::Request {
                version,
                secret,
                client_id,
                ..
            } => (*version, secret, client_id),
        };
        if version != TRANSPORT_VERSION
            || client.is_nil()
            || secret.len() != 64
            || !bool::from(secret.as_bytes().ct_eq(expected.as_bytes()))
        {
            bail!("Private transport authentication/version refused.");
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_versioned_frames_reject_legacy_unknown_and_extra_fields() {
        for json in [
            r#"{"secret":"x","body":[]}"#,
            r#"{"kind":"nativeRegister","version":1,"secret":"x","clientId":"12345678-1234-4234-8234-123456789abc"}"#,
            r#"{"kind":"lifetimeOpen","version":1,"secret":"x","clientId":"12345678-1234-4234-8234-123456789abc","body":[]}"#,
        ] {
            assert!(serde_json::from_str::<Frame>(json).is_err());
        }
        let frame = Frame::LifetimeOpen {
            version: 2,
            secret: "0".repeat(64),
            client_id: uuid::Uuid::new_v4(),
        };
        assert!(frame.authenticate(&"0".repeat(64)).is_err());
    }
}
