//! Opt-in experimental private host contract, never part of the mobile remote allowlist.
use crate::{RequestLimits, TransportFuture, Validate};
use serde::{de::DeserializeOwned, Serialize};

pub const LOCAL_NAMESPACE: &str = "experimental.rust.v1";
/// Private Discover/Submit/Receipt routing. Native registration, claim and ACK are not request methods.
pub const DESKTOP_LAUNCH_NAMESPACE: &str = "experimental.desktop.launch.v1";
pub const SSH_READ_NAMESPACE: &str = "private.ssh.read.v1";
pub const DESKTOP_VIEW_NAMESPACE: &str = "experimental.desktop.view.v1";
pub trait LocalRequest: Serialize + DeserializeOwned + Validate {
    type Response: Serialize + DeserializeOwned + Validate;
    const METHOD: &'static str;
}
/// Transport verifies private endpoint ownership and authentication before sending a frame.
/// The client supplies no URL, peer identity or manager privileged remote method.
pub trait LocalTransport: Send + Sync {
    fn send<'a>(&'a self, body: Vec<u8>, limits: RequestLimits) -> TransportFuture<'a>;
}
