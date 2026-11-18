use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroU32};

pub const MAX_FRAME: usize = 262_144;
pub const MAX_INPUT: usize = 131_072;
pub(crate) const MAX_WRITE: usize = 65_536;
pub(crate) const MAX_PENDING: usize = 128;
pub(crate) const MAX_ID: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterError {
    InvalidLaunch,
    InvalidResource,
    IntegrityMismatch,
    InvalidProtocol,
    InvalidInput,
    InputUnavailable,
    QueueFull,
    StaleGeneration,
    Transport,
    CleanupUnconfirmed,
    Cancelled,
    HelperFailed,
    IsolatedModeUnavailable,
    LoaderPolicyRequired,
}
impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLaunch => "invalid administrator launch specification",
            Self::InvalidResource => "administrator helper resource is missing, redirected or invalid",
            Self::IntegrityMismatch => "administrator helper does not match the trusted build digest",
            Self::InvalidProtocol => "invalid administrator helper protocol",
            Self::InvalidInput => "invalid administrator terminal input or dimensions",
            Self::InputUnavailable => "administrator terminal is not available for input",
            Self::QueueFull => "administrator terminal queue is full",
            Self::StaleGeneration => "stale administrator terminal generation",
            Self::Transport => "administrator helper channel failed; cleanup is unconfirmed",
            Self::CleanupUnconfirmed => "administrator Job cleanup is unconfirmed",
            Self::Cancelled => "administrator approval was cancelled or timed out",
            Self::HelperFailed => "administrator helper failed",
            Self::IsolatedModeUnavailable => "legacy administrator helper cannot suppress account profiles; use verified isolated mode",
            Self::LoaderPolicyRequired => "managed helper loader review is not granted; real administrator launch remains unavailable",
        })
    }
}
impl std::error::Error for AdapterError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Birth(String);
impl Birth {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn parse(value: String) -> Result<Self, AdapterError> {
        if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AdapterError::InvalidProtocol);
        }
        Ok(Self(value))
    }
}
/// Logical PowerShell identity reported over the retained helper's private,
/// authenticated channel. Never substitute the launcher/guardian PID here, and
/// never use these metadata fields as numeric-PID termination authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: NonZeroU32,
    pub birth: Birth,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum Incoming {
    Ready {
        pid: u32,
        birth: String,
    },
    Data {
        data: String,
    },
    Ack {
        id: u64,
    },
    Exit {
        #[serde(rename = "exitCode")]
        exit_code: i32,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ErrorCode {
    Cancelled,
    Failed,
}
pub(crate) enum Message {
    Ready(ProcessIdentity),
    Data(Vec<u8>),
    Ack(u64),
    Exit(i32),
    Error(AdapterError),
}
pub(crate) fn decode(line: &[u8]) -> Result<Message, AdapterError> {
    if line.is_empty() || line.len() > MAX_FRAME {
        return Err(AdapterError::InvalidProtocol);
    }
    let value: Incoming =
        serde_json::from_slice(line).map_err(|_| AdapterError::InvalidProtocol)?;
    Ok(match value {
        Incoming::Ready { pid, birth } => Message::Ready(ProcessIdentity {
            pid: NonZeroU32::new(pid).ok_or(AdapterError::InvalidProtocol)?,
            birth: Birth::parse(birth)?,
        }),
        Incoming::Data { data } => {
            if data.len() > 5464 {
                return Err(AdapterError::InvalidProtocol);
            }
            let bytes = STANDARD
                .decode(&data)
                .map_err(|_| AdapterError::InvalidProtocol)?;
            if bytes.len() > 4096 || STANDARD.encode(&bytes) != data {
                return Err(AdapterError::InvalidProtocol);
            }
            Message::Data(bytes)
        }
        Incoming::Ack { id } if id > 0 && id <= MAX_ID => Message::Ack(id),
        Incoming::Ack { .. } => return Err(AdapterError::InvalidProtocol),
        Incoming::Exit { exit_code } => Message::Exit(exit_code),
        Incoming::Error { code, message } => {
            if message.encode_utf16().count() > 1000 {
                return Err(AdapterError::InvalidProtocol);
            }
            // A compromised helper cannot put input, environment or secrets in
            // user-visible errors. Retain only the closed error-code vocabulary.
            Message::Error(match code {
                ErrorCode::Cancelled => AdapterError::Cancelled,
                ErrorCode::Failed => AdapterError::HelperFailed,
            })
        }
    })
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum Outgoing<'a> {
    Write { id: u64, data: &'a str },
    Resize { cols: u16, rows: u16 },
    Close,
}
pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>, AdapterError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| AdapterError::InvalidInput)?;
    if bytes.len() >= MAX_FRAME {
        return Err(AdapterError::InvalidInput);
    }
    bytes.push(b'\n');
    Ok(bytes)
}
