use serde::{Deserialize, Serialize};
use shellfox_core::{SessionId, TabId};
use uuid::Uuid;

pub const MAX_PENDING_LAUNCHES: usize = 64;
pub const MAX_LAUNCH_RECEIPTS: usize = 1024;
pub const DESKTOP_CHANNEL_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LaunchOwnerFence(pub(super) Uuid);
impl std::fmt::Display for LaunchOwnerFence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::str::FromStr for LaunchOwnerFence {
    type Err = LaunchError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let id = Uuid::parse_str(value).map_err(|_| LaunchError::Invalid)?;
        if id.is_nil() || id.to_string() != value {
            return Err(LaunchError::Invalid);
        }
        Ok(Self(id))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopClientToken {
    pub(super) owner: LaunchOwnerFence,
    pub(super) nonce: Uuid,
}
impl DesktopClientToken {
    pub fn owner(&self) -> LaunchOwnerFence {
        self.owner
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchDisposition {
    ExistingWindow,
    NewWindow,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LaunchAction {
    Show {},
    NewSession { cwd: String },
    OpenSession { session: SessionId },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopLaunchIntent {
    pub request_id: SessionId,
    pub action: LaunchAction,
    pub disposition: LaunchDisposition,
}
impl DesktopLaunchIntent {
    pub fn new(action: LaunchAction, disposition: LaunchDisposition) -> Self {
        Self {
            request_id: SessionId::new(),
            action,
            disposition,
        }
    }
    pub fn validate(&self) -> Result<(), LaunchError> {
        if let LaunchAction::NewSession { cwd } = &self.action {
            shellfox_core::validation::cwd(cwd).map_err(|_| LaunchError::Invalid)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LaunchTarget {
    Show {},
    Session {
        session: SessionId,
        tab: Option<TabId>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopLaunchDelivery {
    pub client: DesktopClientToken,
    pub intent: DesktopLaunchIntent,
    pub target: LaunchTarget,
}
impl DesktopLaunchDelivery {
    pub fn validate(&self) -> Result<(), LaunchError> {
        self.intent.validate()?;
        if self.client.owner.0.is_nil() || self.client.nonce.is_nil() {
            return Err(LaunchError::Invalid);
        }
        match (&self.intent.action, &self.target) {
            (LaunchAction::Show {}, LaunchTarget::Show {})
            | (LaunchAction::NewSession { .. }, LaunchTarget::Session { .. }) => Ok(()),
            (
                LaunchAction::OpenSession { session },
                LaunchTarget::Session {
                    session: target, ..
                },
            ) if session == target => Ok(()),
            _ => Err(LaunchError::Invalid),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchError {
    Invalid,
    StaleOwner,
    StaleClient,
    Conflict,
    Full,
    NoDesktop,
    NotFound,
    ReceiptUnavailable,
    CreationUncertain,
    DeliveryUncertain,
    Rejected,
    Busy,
    Unavailable,
}
impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Invalid desktop launch request.",
            Self::StaleOwner => "Desktop launch ownership changed; request was not replayed.",
            Self::StaleClient => "Desktop client registration is stale.",
            Self::Conflict => "Launch request ID was reused with different input.",
            Self::Full => {
                "Desktop launch queue or receipt bound reached; request was not admitted."
            }
            Self::NoDesktop => "No desktop client is registered on the existing host; headless-to-GUI rendering is not supported and no replacement Host was started.",
            Self::NotFound => "Requested session or directory was not found.",
            Self::ReceiptUnavailable => {
                "Original creation receipt is absent or deleted; no selection was inferred."
            }
            Self::CreationUncertain => {
                "Session creation outcome is uncertain; no automatic retry or focus occurred."
            }
            Self::DeliveryUncertain => {
                "Desktop delivery outcome is uncertain; no automatic replay occurred."
            }
            Self::Rejected => "Desktop action was rejected.",
            Self::Busy => "Another desktop launch is being handled.",
            Self::Unavailable => {
                "Desktop launch transport or configured resources are unavailable."
            }
        })
    }
}
impl std::error::Error for LaunchError {}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    content = "error",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum LaunchState {
    Queued,
    Dispatched,
    Applying,
    Handled,
    Failed(LaunchError),
    Uncertain,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchReceipt {
    pub request_id: SessionId,
    pub state: LaunchState,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchClaim {
    Apply,
    AlreadyHandled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "result",
    content = "error",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum LaunchHandling {
    Handled,
    Failed(LaunchError),
}

/// Actual owner cancellation/identity must be checked on the owner thread.
pub trait LaunchAuthority {
    fn check_owned(&self) -> Result<(), LaunchError>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchRequestKind {
    Fresh,
    Known,
}
pub trait LaunchSessionResolver: LaunchAuthority {
    /// Metadata only. Known creation requests require the durable session receipt;
    /// unavailable/deleted receipts must never recreate or infer a selection.
    fn resolve(
        &mut self,
        intent: &DesktopLaunchIntent,
        kind: LaunchRequestKind,
    ) -> Result<LaunchTarget, LaunchError>;
}
