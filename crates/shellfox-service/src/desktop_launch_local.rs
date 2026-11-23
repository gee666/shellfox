//! A separate PRIVATE local method. Do not add these variants to public remote
//! or mobile APIs. Registration/claim/ACK stay on authenticated desktop routing.
use super::*;
use serde::{Deserialize, Serialize};
use shellfox_protocol::{LocalRequest, Validate, ValidationError};

pub use shellfox_protocol::DESKTOP_LAUNCH_NAMESPACE as DESKTOP_LAUNCH_METHOD;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchRouteInfo {
    pub owner: LaunchOwnerFence,
    pub desktop_registered: bool,
    /// The host executable's [`build_stamp`]. A host from before this field
    /// reports 0, which matches no build.
    #[serde(default)]
    pub build: u64,
}
/// Identifies one build of an executable: its canonical path, size and
/// modification time. A rebuild or an update to another folder changes it.
/// The desktop attaches only to a host whose CLI has the stamp it expects.
pub fn build_stamp(executable: &std::path::Path) -> std::io::Result<u64> {
    use std::hash::{Hash, Hasher};
    let path = std::fs::canonicalize(executable)?;
    let metadata = std::fs::metadata(&path)?;
    let modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    // Fixed keys: the same in every process built by the same toolchain.
    let mut hash = std::hash::DefaultHasher::new();
    (path, metadata.len(), modified).hash(&mut hash);
    Ok(hash.finish().max(1))
}
/// This process's own stamp, computed once.
pub fn own_build_stamp() -> u64 {
    static STAMP: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *STAMP.get_or_init(|| std::env::current_exe().and_then(|exe| build_stamp(&exe)).unwrap_or(0))
}
/// This local DTO cannot be passed through the sealed remote Request API.
/// ```compile_fail
/// fn remote<T: shellfox_protocol::Request>() {}
/// remote::<shellfox_service::desktop_launch::DesktopLaunchRequest>();
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub enum DesktopLaunchRequest {
    Discover {},
    RegisterAttached { owner: LaunchOwnerFence },
    PollAttached { token: DesktopClientToken },
    ClaimAttached { delivery: DesktopLaunchDelivery },
    CompleteAttached { delivery: DesktopLaunchDelivery, handling: LaunchHandling },
    UnregisterAttached { token: DesktopClientToken },
    Submit {
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
    },
    Receipt {
        owner: LaunchOwnerFence,
        request_id: SessionId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DesktopLaunchReply {
    Route(LaunchRouteInfo),
    Receipt(LaunchReceipt),
    Registered(DesktopClientToken),
    Delivery(Option<DesktopLaunchDelivery>),
    Claim(LaunchClaim),
    Unregistered {},
}
impl DesktopLaunchRequest {
    pub(crate) fn is_attached_renderer(&self) -> bool {
        matches!(self, Self::RegisterAttached { .. } | Self::PollAttached { .. } | Self::ClaimAttached { .. }
            | Self::CompleteAttached { .. } | Self::UnregisterAttached { .. })
    }
}
impl DesktopClientToken {
    fn validate_wire(&self) -> Result<(), ValidationError> {
        shellfox_protocol::validation::ensure(!self.owner.0.is_nil() && !self.nonce.is_nil())
    }
}
impl Validate for DesktopLaunchRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Discover {} => Ok(()),
            Self::RegisterAttached { owner } => shellfox_protocol::validation::ensure(!owner.0.is_nil()),
            Self::PollAttached { token } | Self::UnregisterAttached { token } => token.validate_wire(),
            Self::ClaimAttached { delivery } | Self::CompleteAttached { delivery, .. } => delivery.validate().map_err(|_| ValidationError),
            Self::Submit { owner, intent, .. } => {
                if owner.0.is_nil() {
                    return Err(ValidationError);
                }
                intent.validate().map_err(|_| ValidationError)
            }
            Self::Receipt { owner, .. } => {
                if owner.0.is_nil() {
                    Err(ValidationError)
                } else {
                    Ok(())
                }
            }
        }
    }
}
impl Validate for DesktopLaunchReply {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Route(route) if route.owner.0.is_nil() => Err(ValidationError),
            Self::Registered(token) => token.validate_wire(),
            Self::Delivery(Some(delivery)) => delivery.validate().map_err(|_| ValidationError),
            _ => Ok(()),
        }
    }
}
impl LocalRequest for DesktopLaunchRequest {
    type Response = DesktopLaunchReply;
    const METHOD: &'static str = DESKTOP_LAUNCH_METHOD;
}
impl DesktopLaunchRouter {
    pub fn handle_local(
        &mut self,
        request: DesktopLaunchRequest,
        authority: &impl LaunchAuthority,
    ) -> Result<DesktopLaunchReply, LaunchError> {
        request.validate().map_err(|_| LaunchError::Invalid)?;
        match request {
            DesktopLaunchRequest::Discover {} => {
                self.route_info(authority).map(DesktopLaunchReply::Route)
            }
            DesktopLaunchRequest::Submit {
                owner,
                intent,
                submission,
            } => {
                self.check(owner, authority)?;
                // Submit only after a verified renderer registered. The launcher
                // can attach and register when a headless host has no renderer.
                if !self.route_info(authority)?.desktop_registered {
                    return Err(LaunchError::NoDesktop);
                }
                match submission {
                    LaunchRequestKind::Fresh => self.enqueue(owner, intent, authority),
                    LaunchRequestKind::Known => self.resume(owner, intent, authority),
                }
                .map(DesktopLaunchReply::Receipt)
            }
            DesktopLaunchRequest::Receipt { owner, request_id } => self
                .receipt(owner, request_id, authority)
                .map(DesktopLaunchReply::Receipt),
            _ => Err(LaunchError::Invalid), // Requires the verified local lifetime dispatcher.
        }
    }
}
