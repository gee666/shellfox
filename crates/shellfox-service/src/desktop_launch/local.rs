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
    /// The host's [`build_stamp`], or 0 for a host that set none or predates
    /// this field. 0 matches no build.
    #[serde(default)]
    pub build: u64,
}
/// Identifies one build: the stamp of its `SHELLFOX_BUILD_ID`. Never 0.
pub fn build_stamp(build_id: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    // Fixed keys: the same in every binary built by the same toolchain.
    let mut hash = std::hash::DefaultHasher::new();
    build_id.hash(&mut hash);
    hash.finish().max(1)
}
static HOST_BUILD: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
/// Set once by the host process before it serves, from its own build id.
pub fn set_host_build(build_id: &str) {
    let _ = HOST_BUILD.set(build_stamp(build_id));
}
pub(crate) fn host_build() -> u64 {
    HOST_BUILD.get().copied().unwrap_or(0)
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
