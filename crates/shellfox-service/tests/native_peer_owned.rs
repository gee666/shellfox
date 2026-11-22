//! Actual isolated tsnet/WhoIs -> same Host SQL approval -> Manager -> Runtime.
#![cfg(feature = "experimental-shellfox-native")]
#[path = "../../native_app_composition.rs"]
pub mod application;
pub use shellfox_service::peer::consumer;
#[path = "../src/remote_network_health.rs"]
mod health;
#[path = "support/native_peer_owned.rs"]
mod owned;
#[path = "support/native_bootstrap_owned.rs"]
pub mod support;
#[path = "support/native_peer_terminal.rs"]
mod terminal;
pub use shellfox_service::peer::transport;
use anyhow::{ensure, Context, Result};
use shellfox_protocol::{manager_v2 as m, AuthenticatedPeerId, CancellationToken};
use shellfox_service::{
    native::{NativeAction, NativeClient, NativeResponse},
    remote::{ControlServerOrigin, RemoteBootResources, RemoteControlState, RemoteRequest},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

fn sdk<T>(value: std::result::Result<T, shellfox_mobile::ManagerRecoveryError>) -> Result<T> {
    value.map_err(|e| anyhow::anyhow!("Actual SDK operation failed: {e:?}"))
}
#[path = "support/native_peer_network.rs"]
mod network;
use network::{control, online, options, state};
#[test]
fn actual_two_go_nodes_whois_sql_approval_manager_lease_recovery_input_and_revoke() -> Result<()> {
    let fixture = support::Fixture::new()?;
    let a = fixture.data("real-peer-a");
    let b = fixture.data("real-peer-b");
    let local_data = a.clone();
    let target_data = b.clone();
    let helper = fixture.helper.clone();
    let digest = fixture.digest;
    let (control, expected_control) = control()?;
    let options = [
        options(&a, "owned-peer-a-loginfixed", control.clone())?,
        options(&b, "owned-peer-b-loginfixed", control)?,
    ];
    owned::two(&fixture, &a, &b, options, |owner_a, owner_b| async move {
        let repository = owner_b.approval_repository(CancellationToken::new());
        let native_a = NativeClient::new(owner_a);
        let native_b = NativeClient::new(owner_b);
        native_a
            .execute(NativeAction::Remote(RemoteRequest::Configure {
                enabled: true,
                hostname: "owned-peer-a-loginfixed".into(),
            }))
            .await?;
        native_b
            .execute(NativeAction::Remote(RemoteRequest::Configure {
                enabled: true,
                hostname: "owned-peer-b-loginfixed".into(),
            }))
            .await?;
        let (peer_b, peer_a) = tokio::try_join!(
            online(&native_a, "owned-peer-b-loginfixed", &expected_control),
            online(&native_b, "owned-peer-a-loginfixed", &expected_control)
        )?;
        eprintln!("Both actual Go nodes online; current authenticated peer maps agree.");
        let id = m::Id::new();
        let manager = sdk(shellfox_mobile::ManagerRecovery::native_client(
            peer_b.clone(),
            id,
            transport::ServiceTransport::new(native_a.clone()),
        ))?;
        ensure!(
            matches!(
                manager
                    .discover(shellfox_client::manager_v2::FallbackPolicy::Never)
                    .await?,
                shellfox_client::manager_v2::DiscoveryOutcome::Available(_)
            ),
            "Actual Manager/Native0 offer unavailable"
        );
        let attempt = m::Id::new();
        let pending = manager.pair(attempt).await?;
        ensure!(
            matches!(pending,m::Reply::Failure(ref e) if e.code==shellfox_protocol::ErrorCode::ApprovalRequired),
            "Unapproved real peer unexpectedly paired or failed without a pending decision"
        );
        ensure!(
            state(&native_b)
                .await?
                .pending
                .iter()
                .any(|p| p.peer.id == peer_a.as_str()),
            "Actual WhoIs peer did not reach target pending approval"
        );
        native_b
            .execute(NativeAction::Remote(RemoteRequest::Approve {
                peer_id: peer_a.as_str().into(),
            }))
            .await?;
        let approved = repository
            .load()
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
        ensure!(
            approved.approvals.len() == 1
                && approved.approvals[0].peer_id == peer_a.as_str()
                && approved.approvals[0].policy
                    == shellfox_core::ApprovalStorePolicy::ControlFullAccountV1,
            "Same Host SQL commit absent"
        );
        consumer::reply(manager.pair(attempt).await?)?;
        ensure!(
            manager.binding_witness().is_ok(),
            "Actual binding absent after confirmed approval continuation"
        );
        let snapshot = consumer::reply(
            manager
                .execute(&m::GetSnapshot(shellfox_protocol::GetSnapshot {}))
                .await?,
        )?;
        ensure!(
            snapshot.entries.is_empty(),
            "Fresh target unexpectedly had sessions"
        );
        // Separate logical CLI discovery shares the SAME original Go/Host;
        // it does not PAIR, replace this Manager binding, or create another node.
        let local =
            shellfox_service::local::LocalClient::connect(&local_data, &helper, digest).await?;
        {
            let cli = sdk(shellfox_mobile::ManagerRecovery::native_client(
                peer_b.clone(),
                m::Id::new(),
                transport::ServiceTransport::new(&local),
            ))?;
            ensure!(
                matches!(
                    cli.discover(shellfox_client::manager_v2::FallbackPolicy::Never)
                        .await?,
                    shellfox_client::manager_v2::DiscoveryOutcome::Available(_)
                ),
                "Actual LocalClient status/body bridge failed"
            );
        }
        local.disconnect().await?;
        let mut recovery = terminal::exercise(&native_b, manager.clone(), &target_data).await?;
        let withheld = sdk(recovery.admit_input(
            m::Input::Bytes {
                encoding: m::Encoding::Base64,
                data: shellfox_protocol::encode_chunk(b"echo MUST_NOT_ADMIT_AFTER_REVOKE\r")?,
                origin: m::BytesOrigin::Key,
            },
            m::InputHints {
                user_initiated: true,
                activity_hint: false,
                settled_at: None,
            },
        ))?;
        // This is only SDK admission, not a sent request or acceptance receipt.
        // Revoke must deny the real old lease before its eventual send.
        native_b
            .execute(NativeAction::Remote(RemoteRequest::Revoke {
                peer_id: peer_a.as_str().into(),
            }))
            .await?;
        let revoked = repository
            .load()
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
        ensure!(
            revoked.approvals.is_empty() && revoked.revision > approved.revision,
            "Same Host SQL durable revoke absent"
        );
        ensure!(
            state(&native_a)
                .await?
                .peers
                .iter()
                .any(|p| p.id == peer_b.as_str() && p.online),
            "Revocation test lost the actual Go peer"
        );
        let denied = recovery.execute_input(withheld).await;
        let authoritative = match denied {
            Ok(m::Reply::Failure(e)) => matches!(
                e.code,
                shellfox_protocol::ErrorCode::AccessDenied
                    | shellfox_protocol::ErrorCode::ApprovalRequired
                    | shellfox_protocol::ErrorCode::TargetLost
            ),
            Err(shellfox_mobile::ManagerRecoveryError::Client(
                shellfox_client::manager_v2::ClientError::Host(e),
            )) => matches!(
                e.code,
                shellfox_protocol::ErrorCode::AccessDenied
                    | shellfox_protocol::ErrorCode::ApprovalRequired
            ),
            Err(shellfox_mobile::ManagerRecoveryError::Client(
                shellfox_client::manager_v2::ClientError::Transport(
                    shellfox_protocol::TransportError::Rejected { status: 403 },
                ),
            )) => true,
            _ => false,
        };
        ensure!(
            authoritative,
            "Revoke did not produce an authoritative denial for the actual old lease"
        );
        sdk(recovery.close())?;
        eprintln!("Real WhoIs, pending/approve, Host SQL, Manager binding, lease/recovery/input and revoke observed.");
        Ok(())
    })?;
    Ok(())
}
