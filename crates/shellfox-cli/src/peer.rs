//! Native-only consumer. Default arguments are rejected before any Host connect;
//! only artifacts with the actual verified receiver expose this module.
#[path = "peer_commands.rs"]
mod native;
use crate::peer_args::PeerArgs;
use anyhow::Result;
use shellfox_service::local::LocalClient;
pub(crate) async fn run(client: &LocalClient, args: PeerArgs) -> Result<i32> {
    native::run(client, args).await
}
