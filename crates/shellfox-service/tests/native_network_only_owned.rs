//! Captured production libraries; actual network/WhoIs prefix ONLY, no SDK parser.
#![cfg(feature="experimental-shellfox-native")]
#[path="../../native_app_composition.rs"] pub mod application;
#[path="../../native_peer_transport.rs"] pub mod transport;
#[path="support/native_bootstrap_owned.rs"] pub mod support;
#[path="support/native_peer_owned.rs"] mod owned;
#[path="../src/remote_network_health.rs"] mod health;
#[path="support/native_peer_network.rs"] mod network;
use anyhow::{Context,Result,ensure};
use shellfox_protocol::{AuthenticatedPeerId,manager_v2 as m};
use shellfox_service::{native::{NativeAction,NativeClient,NativeResponse},remote::{RemoteRequest,RemoteControlState,RemoteBootResources,ControlServerOrigin}};
use std::{path::{Path,PathBuf},time::{Duration,Instant}};

#[test]
fn actual_two_go_nodes_network_only_whois_manager_pending_then_original_stop()->Result<()>{
    let mut fixture=support::Fixture::new()?;
    let a=fixture.data("network-only-a");let b=fixture.data("network-only-b");
    let(control,expected)=network::control()?;
    let options=[network::options(&a,"owned-peer-a-netonly",control.clone())?,network::options(&b,"owned-peer-b-netonly",control)?];
    owned::two(&fixture,&a,&b,options,|owner_a,owner_b|async move{
        let native_a=NativeClient::new(owner_a);let native_b=NativeClient::new(owner_b);
        native_a.execute(NativeAction::Remote(RemoteRequest::Configure{enabled:true,hostname:"owned-peer-a-netonly".into()})).await?;
        native_b.execute(NativeAction::Remote(RemoteRequest::Configure{enabled:true,hostname:"owned-peer-b-netonly".into()})).await?;
        let(peer_b,peer_a)=tokio::try_join!(network::online(&native_a,"owned-peer-b-netonly",&expected),network::online(&native_b,"owned-peer-a-netonly",&expected))?;
        eprintln!("NETWORK ONLY: both real Go nodes online, intended authenticated peer IDs observed.");
        // Advertise the actually verified local codec; no terminal/engine/view is
        // created, reset, submitted, installed, pumped or acknowledged here.
        let verified=shellfox_ghostty_vt::extension::VerifiedNativeExtension::verify()?;
        let identity=String::from_utf8(verified.identity().as_bytes().to_vec())?;
        let codec=m::Native0Codec::new(m::EngineIdentity::try_from(identity)?);
        let client=shellfox_client::manager_v2::ManagerClient::native0(peer_b,m::Id::new(),transport::ServiceTransport::new(native_a),codec)?;
        ensure!(matches!(client.discover(shellfox_client::manager_v2::FallbackPolicy::Never).await?,shellfox_client::manager_v2::DiscoveryOutcome::Available(_)),"Actual Manager discovery unavailable");
        let reply=client.pair(m::Id::new()).await?;
        ensure!(matches!(reply,m::Reply::Failure(ref e) if e.code==shellfox_protocol::ErrorCode::ApprovalRequired),"Unapproved actual peer did not return explicit pending approval");
        let target=network::state(&native_b).await?;
        ensure!(target.pending.iter().any(|p|p.peer.id==peer_a.as_str())&&target.approvals.is_empty(),"Actual WhoIs identity did not reach pending state or gained authority");
        eprintln!("NETWORK ONLY: real tsnet/WhoIs Manager discovery and pending approval observed; no terminal/SDK security acceptance.");
        Ok(())
    })?;
    fixture.success();Ok(())
}
