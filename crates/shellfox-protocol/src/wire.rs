use crate::{
    validation::{ensure, text},
    AppError, ClientId, Validate, ValidationError,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endpoint {
    Discover,
    Pair,
    Request,
    Events,
    Disconnect,
    DiscoverV2,
    PairV2,
    RequestV2,
    EventsV2,
    DisconnectV2,
}
impl Endpoint {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Discover => "/v1/discover",
            Self::Pair => "/v1/pair",
            Self::Request => "/v1/request",
            Self::Events => "/v1/events",
            Self::Disconnect => "/v1/disconnect",
            Self::DiscoverV2 => "/v2/discover",
            Self::PairV2 => "/v2/pair",
            Self::RequestV2 => "/v2/request",
            Self::EventsV2 => "/v2/events",
            Self::DisconnectV2 => "/v2/disconnect",
        }
    }
}

/// Destination only. Native code supplies authenticated *caller* identity itself.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AuthenticatedPeerId(String);
impl AuthenticatedPeerId {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        ensure(
            !value.is_empty()
                && value.len() <= 200
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b)),
        )?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for AuthenticatedPeerId {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<AuthenticatedPeerId> for String {
    fn from(value: AuthenticatedPeerId) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewIdentity {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub client_id: ClientId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairResponse {
    pub paired: bool,
    pub terminal_control: String,
}
impl Validate for PairResponse {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.paired && self.terminal_control == "exclusive-v1")
    }
}

/// These permissions are decisions made by the host, not claims sent by the client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    Unknown,
    AwaitingApproval,
    Approved,
    Rejected,
}
impl Permission {
    pub fn from_error(error: &AppError) -> Self {
        match error.code {
            crate::ErrorCode::ApprovalRequired => Self::AwaitingApproval,
            crate::ErrorCode::AccessDenied => Self::Rejected,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangedEvent {
    pub revision: u64,
    pub reason: ChangeReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub select_session_id: Option<crate::SessionId>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeReason {
    Sessions,
    Settings,
    Native,
}
impl Validate for ChangedEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.revision <= crate::MAX_SAFE_INTEGER)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum HostEvent {
    Changed { event: ChangedEvent },
    Terminal { event: crate::TerminalEvent },
}
impl Validate for HostEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Changed { event } => event.validate(),
            Self::Terminal { event } => event.validate(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollResponse {
    pub events: Vec<HostEvent>,
}
impl Validate for PollResponse {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.events.len() <= 1024)?;
        for event in &self.events {
            event.validate()?;
        }
        Ok(())
    }
}
impl Validate for String {
    fn validate(&self) -> Result<(), ValidationError> {
        text(self, 32767)
    }
}
