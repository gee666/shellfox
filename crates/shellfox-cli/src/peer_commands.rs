//! Manager operations over an already verified, owner-provided transport.
//! This layer does not open network sockets or create a host/node.
#[path = "peer_attach.rs"]
mod attach;
pub(super) use shellfox_service::peer::consumer as connection;
#[path = "peer_frame.rs"]
mod peer_frame;
#[path = "peer_keys.rs"]
mod peer_keys;
#[path = "../../native_peer_pending.rs"]
mod peer_pending;
use shellfox_service::peer::transport;
use crate::peer_args::{PeerArgs, PeerCommand};
use anyhow::{Result, ensure};
use connection::PeerConnection;
use shellfox_client::manager_v2::ManagerClient;
use shellfox_protocol::{OutboundTransport, Validate, manager_v2 as m};
use std::sync::Arc;

pub(super) async fn run(
    local: &shellfox_service::local::LocalClient,
    args: PeerArgs,
) -> Result<i32> {
    let id = m::Id::new();
    let manager = shellfox_mobile::ManagerRecovery::native_client(
        args.peer.clone(),
        id,
        transport::ServiceTransport::new(local),
    )
    .map_err(|error| anyhow::anyhow!("Native Manager receiver unavailable: {error:?}"))?;
    consume(manager, id, args).await
}
pub(crate) async fn consume<T: OutboundTransport>(
    client: Arc<ManagerClient<T>>,
    client_id: m::Id,
    args: PeerArgs,
) -> Result<i32> {
    ensure!(
        client.peer() == &args.peer,
        "Manager client belongs to a different peer."
    );
    let connection = Arc::new(PeerConnection::new(client, client_id));
    let result = command(&connection, args.command).await;
    let withdrawal = connection.disconnect().await;
    // Always withdraw this logical CLI client's views, even after command failure.
    // Do not stop the host or delete its terminals.
    match (result, withdrawal) {
        (Err(error), Err(withdrawal)) => {
            Err(error.context(format!("Peer withdrawal also failed: {withdrawal:#}")))
        }
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(code), Ok(())) => Ok(code),
    }
}
async fn command<T: OutboundTransport>(
    connection: &Arc<PeerConnection<T>>,
    command: PeerCommand,
) -> Result<i32> {
    let origin = connection.connect().await?;
    match command {
        PeerCommand::Sessions => {
            let snapshot = connection.snapshot(&origin).await?;
            // Preserve unavailable entries, Known/Unavailable facts and coverage;
            // don't turn the native catalog back into the old local DTO.
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        PeerCommand::Tabs { session } => {
            let snapshot = connection.snapshot(&origin).await?;
            if let Some(session) = session {
                let entry = snapshot
                    .entries
                    .iter()
                    .find(|entry| entry.id() == session)
                    .ok_or_else(|| anyhow::anyhow!("Session is not in this peer's catalog."))?;
                println!("{}", serde_json::to_string_pretty(entry)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&snapshot.entries)?);
            }
        }
        PeerCommand::Create { cwd, title } => {
            let request = shellfox_protocol::CreateSession {
                cwd,
                title,
                request_id: shellfox_protocol::SessionId::new(),
            };
            request.validate()?;
            let actual = connection
                .execute(&origin, &m::CreateSession(request))
                .await?;
            // The returned ID is authority. request_id is not a created session ID.
            println!("{}", serde_json::to_string_pretty(&actual)?);
        }
        PeerCommand::Attach { tab, takeover } => {
            let snapshot = connection.snapshot(&origin).await?;
            ensure!(snapshot.entries.iter().any(|entry| matches!(entry, m::CatalogEntry::Session { value } if value.tabs.iter().any(|item| item.id == tab))), "Tab is not in this peer's catalog.");
            attach::run(connection.clone(), tab, takeover).await?;
        }
    }
    Ok(0)
}
