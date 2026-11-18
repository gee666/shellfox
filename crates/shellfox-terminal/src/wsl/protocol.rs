use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const FRAME_BYTES: usize = 96 * 1024;
pub const DATA_BYTES: usize = 4096;
pub const CREDIT_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WslError {
    InvalidRequest,
    Unavailable,
    Protocol,
    Deadline,
    Busy,
    CleanupUnconfirmed,
    Io,
}
impl std::fmt::Display for WslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "Invalid WSL configuration or request.",
            Self::Unavailable => "Verified WSL guest capability unavailable.",
            Self::Protocol => "Invalid WSL guardian protocol.",
            Self::Deadline => "WSL guardian deadline exceeded.",
            Self::Busy => "WSL guardian request queue is full.",
            Self::CleanupUnconfirmed => {
                "WSL guest cleanup remains unconfirmed; ownership retained."
            }
            Self::Io => "WSL transport stopped.",
        })
    }
}
impl std::error::Error for WslError {}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuardianAnchor {
    pub boot_id: String,
    pub guardian_pid: u32,
    pub guardian_ticks: String,
    pub marker: String,
}
impl GuardianAnchor {
    pub(crate) fn validate(&self) -> Result<(), WslError> {
        if !boot_id(&self.boot_id)
            || !(2..=i32::MAX as u32).contains(&self.guardian_pid)
            || !decimal(&self.guardian_ticks)
            || !token(&self.marker)
        {
            return Err(WslError::Protocol);
        }
        Ok(())
    }
    pub(crate) fn matches(&self, identity: &GuestIdentity) -> bool {
        self.boot_id == identity.boot_id
            && self.guardian_pid == identity.guardian_pid
            && self.guardian_ticks == identity.guardian_ticks
            && self.marker == identity.marker
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuestIdentity {
    pub boot_id: String,
    pub guardian_pid: u32,
    pub guardian_ticks: String,
    pub root_pid: u32,
    pub root_ticks: String,
    pub session: u32,
    pub marker: String,
}
impl GuestIdentity {
    pub(crate) fn validate(&self) -> Result<(), WslError> {
        if !boot_id(&self.boot_id)
            || !(2..=i32::MAX as u32).contains(&self.guardian_pid)
            || !(2..=i32::MAX as u32).contains(&self.root_pid)
            || self.guardian_pid == self.root_pid
            || self.session != self.root_pid
            || !decimal(&self.guardian_ticks)
            || !decimal(&self.root_ticks)
            || !token(&self.marker)
        {
            return Err(WslError::Protocol);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuestStatus {
    pub identity: GuestIdentity,
    pub owned: u16,
    pub cleanup_confirmed: bool,
    #[serde(deserialize_with = "nullable_exit")]
    pub root_exit: Option<u32>,
}
fn nullable_exit<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(d)
}
impl GuestStatus {
    pub(crate) fn validate(&self, identity: &GuestIdentity) -> Result<(), WslError> {
        self.identity.validate()?;
        if &self.identity != identity
            || self.owned > 256
            || (self.cleanup_confirmed && self.owned != 0)
            || self.root_exit.is_some_and(|code| code > 255)
        {
            return Err(WslError::Protocol);
        }
        Ok(())
    }
}
/// Retain as a credential. It is not a PID-based recovery instruction.
#[derive(Clone)]
pub struct RecoveryEvidence {
    pub(crate) settings: super::GuestSettings,
    pub(crate) distro: super::WslProfileId,
    pub(crate) secret: super::WslSecret,
    pub(crate) endpoint: String,
    pub(crate) anchor: GuardianAnchor,
    pub(crate) identity: Option<GuestIdentity>,
}
impl RecoveryEvidence {
    /// Reconstruct from caller-owned private credential storage. This is not
    /// liveness proof: STATUS/CLOSE must authenticate the current guest endpoint.
    pub fn restore(
        settings: super::GuestSettings,
        distro: super::WslProfileId,
        secret: super::WslSecret,
        endpoint: String,
        anchor: GuardianAnchor,
        identity: Option<GuestIdentity>,
    ) -> Result<Self, WslError> {
        super::config::guest_path(&endpoint)?;
        anchor.validate().map_err(|_| WslError::InvalidRequest)?;
        if endpoint.len() > 104
            || !endpoint.ends_with("/control.sock")
            || anchor.marker.eq_ignore_ascii_case(&secret.encoded())
        {
            return Err(WslError::InvalidRequest);
        }
        if let Some(identity) = &identity {
            identity.validate().map_err(|_| WslError::InvalidRequest)?;
            if !anchor.matches(identity) {
                return Err(WslError::InvalidRequest);
            }
        }
        Ok(Self {
            settings,
            distro,
            secret,
            endpoint,
            anchor,
            identity,
        })
    }
    pub fn identity(&self) -> Option<&GuestIdentity> {
        self.identity.as_ref()
    }
    pub fn anchor(&self) -> &GuardianAnchor {
        &self.anchor
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}
impl std::fmt::Debug for RecoveryEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryEvidence([private])")
    }
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Frame {
    Capability {
        protocol: u8,
        boot: String,
        pidfd: bool,
        subreaper: bool,
        peercred: bool,
    },
    Prepared {
        anchor: GuardianAnchor,
        endpoint: String,
    },
    Ready {
        status: GuestStatus,
        endpoint: String,
    },
    Stopped {
        status: GuestStatus,
    },
    Aborted {
        anchor: GuardianAnchor,
        cleanup_confirmed: bool,
        owned: u16,
    },
    InputAck {
        bytes: usize,
    },
    Data {
        sequence: u64,
        data: String,
    },
    Reply {
        id: u64,
        status: GuestStatus,
    },
    Eof {},
    Error {
        code: String,
    },
}
impl Frame {
    pub(crate) fn data(
        sequence: u64,
        encoded: &str,
        previous: &mut u64,
    ) -> Result<Vec<u8>, WslError> {
        if sequence != *previous + 1 || encoded.len() > DATA_BYTES.div_ceil(3) * 4 {
            return Err(WslError::Protocol);
        }
        let bytes = STANDARD.decode(encoded).map_err(|_| WslError::Protocol)?;
        if bytes.is_empty() || bytes.len() > DATA_BYTES {
            return Err(WslError::Protocol);
        }
        *previous = sequence;
        Ok(bytes)
    }
}
pub(crate) fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), WslError> {
    let bytes = serde_json::to_vec(value).map_err(|_| WslError::InvalidRequest)?;
    if bytes.is_empty() || bytes.len() > FRAME_BYTES {
        return Err(WslError::InvalidRequest);
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| writer.write_all(&bytes))
        .and_then(|_| writer.flush())
        .map_err(|_| WslError::Io)
}
pub(crate) fn read_frame(reader: &mut impl Read) -> Result<Frame, WslError> {
    let mut header = [0; 4];
    reader.read_exact(&mut header).map_err(|_| WslError::Io)?;
    let size = u32::from_be_bytes(header) as usize;
    if size == 0 || size > FRAME_BYTES {
        return Err(WslError::Protocol);
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).map_err(|_| WslError::Io)?;
    serde_json::from_slice(&bytes).map_err(|_| WslError::Protocol)
}
pub(crate) fn token(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(crate) fn boot_id(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(index, b)| {
            if [8, 13, 18, 23].contains(&index) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn decimal(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 20
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u64>().is_ok_and(|value| value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framed_data_is_exact_ordered_and_bounded_not_terminal_control() {
        let mut sequence = 0;
        let raw = b"\x1b[31m{\"action\":\"close\"}\x1b[0m\x00";
        assert_eq!(
            Frame::data(1, &STANDARD.encode(raw), &mut sequence).unwrap(),
            raw
        );
        assert_eq!(
            Frame::data(1, &STANDARD.encode(raw), &mut sequence),
            Err(WslError::Protocol)
        );
        assert_eq!(
            Frame::data(2, "!not-base64", &mut sequence),
            Err(WslError::Protocol)
        );
        assert_eq!(
            Frame::data(2, &STANDARD.encode(vec![0; DATA_BYTES + 1]), &mut sequence),
            Err(WslError::Protocol)
        );
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &serde_json::json!({"type":"eof"})).unwrap();
        assert!(matches!(
            read_frame(&mut bytes.as_slice()),
            Ok(Frame::Eof {})
        ));
        assert!(matches!(
            read_frame(&mut ((FRAME_BYTES + 1) as u32).to_be_bytes().as_slice()),
            Err(WslError::Protocol)
        ));
        for raw in [
            r#"{"type":"eof","data":"unexpected"}"#,
            r#"{"type":"data","sequence":1,"sequence":2,"data":"QQ=="}"#,
        ] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(raw.len() as u32).to_be_bytes());
            bytes.extend_from_slice(raw.as_bytes());
            assert!(matches!(
                read_frame(&mut bytes.as_slice()),
                Err(WslError::Protocol)
            ));
        }
    }
}
