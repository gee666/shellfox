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
}
impl Validate for DesktopLaunchRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Discover {} => Ok(()),
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
                // Current slice supports handoff to the existing in-process UI
                // only. A private request cannot bootstrap rendering on a headless owner.
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
        }
    }
}
