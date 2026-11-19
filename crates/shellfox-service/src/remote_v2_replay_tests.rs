use super::*;
use crate::{NativeFactoryPolicy, local::PrivateRequest, owner::ServiceOwner};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use shellfox_client::{manager_v2::ManagerClient, observer_v2::ObserverClient};
use shellfox_protocol::{
    AuthenticatedPeerId, Endpoint, OutboundTransport, RequestLimits, TransportError,
    TransportFuture, automation::*, manager_v2 as m, observer_v2 as o,
};
use shellfox_transport::incoming::{PeerIdentity, SignedHeaders};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};
use uuid::Uuid;

const MARKER: &str = "COMPLETED_RAW_V2_REPLAY";
const SECRET: &str = "test-only-replay-fixture-secret-000";
const PEER: &str = "n-replay-fixture";
// Base64url of the signed fixture peer ID and hostname, never supplied by the DTO.
const HEADER: &str = "eyJpZCI6Im4tcmVwbGF5LWZpeHR1cmUiLCJob3N0bmFtZSI6InJlcGxheS1maXh0dXJlIn0";

#[derive(Clone)]
struct SignedFixture(Arc<shellfox_remote::Backend>, bool);

impl OutboundTransport for SignedFixture {
    fn send<'a>(
        &'a self,
        _peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .to_string();
            let message = format!(
                "{timestamp}\n{HEADER}\nPOST\n{}\n{:x}",
                endpoint.path(),
                Sha256::digest(&body)
            );
            let pad = |byte| {
                let mut pad = [byte; 64];
                for (slot, key) in pad.iter_mut().zip(SECRET.bytes()) {
                    *slot ^= key;
                }
                pad
            };
            let inner = Sha256::new()
                .chain_update(pad(0x36))
                .chain_update(message)
                .finalize();
            let signature = format!(
                "{:x}",
                Sha256::new()
                    .chain_update(pad(0x5c))
                    .chain_update(inner)
                    .finalize()
            );
            let headers = SignedHeaders {
                peer: HEADER,
                timestamp: &timestamp,
                signature: &signature,
            };
            let send = async {
                if self.1 {
                    let reply = self
                        .0
                        .handle_manager_v2_signed(endpoint, &body, headers)
                        .await;
                    (reply.status, reply.body)
                } else {
                    let reply = self
                        .0
                        .handle_observer_v2_signed(endpoint, &body, headers)
                        .await;
                    (reply.status, reply.body)
                }
            };
            let (status, body) = tokio::select! {
                _ = limits.cancel.cancelled() => return Err(TransportError::Cancelled),
                result = tokio::time::timeout_at(limits.deadline.into(), send) => result.map_err(|_| TransportError::Deadline)?,
            };
            if status != 200 {
                return Err(TransportError::Rejected { status });
            }
            if body.len() > limits.max_response_bytes {
                return Err(TransportError::TooLarge);
            }
            Ok(body)
        })
    }
}

#[tokio::test]
#[ignore = "requires SHELLFOX_RAW_TEST_HELPER, a compiled owned Windows raw fixture"]
async fn completed_replay_is_observer_only_in_both_v2_profiles() -> Result<()> {
    let supervisor =
        PathBuf::from(std::env::var_os("SHELLFOX_RAW_TEST_HELPER").context("owned raw fixture")?);
    let data = crate::tests::scratch();
    let fixture = crate::native_metadata_fixture::Fixture::new();
    let (sender, receiver) = shellfox_remote::owner_channel(32);
    let dispatcher =
        RemoteDispatcher::new(receiver, RemoteState::new(Default::default(), None, None));
    let mut owner = ServiceOwner::start(
        data.path().into(),
        supervisor.clone(),
        fixture.lost.clone(),
        dispatcher,
    )
    .await?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let actor = owner.client(Uuid::new_v4());
        let store = shellfox_remote::PrivateApprovalStore::retained_native(
            fixture.files.clone(),
            fixture.lost.clone(),
        );
        let app = |error: AppError| anyhow::anyhow!("{:?}: {}", error.code, error.message);
        store
            .bind_repository(actor.approval_repository(fixture.lost.clone()))
            .map_err(app)?;
        store.initialize_read_only().map_err(app)?;
        let approvals = shellfox_remote::ApprovalControl::load(&store)
            .await
            .map_err(app)?;
        approvals.set_enabled(true).map_err(app)?;
        owner.activate().await?;
        actor
            .install_observer_factory_policy(NativeFactoryPolicy::ExistingManagerStandardCatalog)
            .await?;
        let peer = PeerIdentity {
            id: PEER.into(),
            hostname: "replay-fixture".into(),
        };
        approvals
            .approve(
                &store,
                shellfox_remote::Approval {
                    peer_id: peer.id,
                    hostname: peer.hostname,
                    approved_at_millis: 1,
                    permission: shellfox_remote::ViewPermission::Write,
                    file_roots: Vec::new(),
                },
            )
            .await
            .map_err(app)?;
        let registration = actor.desktop_register().await?;
        let epoch = actor.open_lifetime(Uuid::new_v4()).await?;
        let id = Uuid::new_v4();
        actor
            .private_request_in_epoch(
                PrivateRequest::AutomationStart(Box::new(AutomationStart {
                    request_id: id,
                    executable: supervisor.to_str().context("Unicode fixture")?.into(),
                    args: vec!["describe".into(), MARKER.into()],
                    cwd: data.path().to_str().context("Unicode test data")?.into(),
                    env: Default::default(),
                    title: Some("completed raw".into()),
                    detached: true,
                })),
                epoch,
            )
            .await?;
        loop {
            let runs: AutomationListing = serde_json::from_value(
                actor
                    .private_request_in_epoch(
                        PrivateRequest::AutomationList(AutomationList {}),
                        epoch,
                    )
                    .await?,
            )?;
            if runs
                .0
                .iter()
                .any(|run| run.id == id && run.state == AutomationState::Exited)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let run: AutomationRun = serde_json::from_value(
            actor
                .private_request_in_epoch(
                    PrivateRequest::AutomationShow(AutomationShow { id }),
                    epoch,
                )
                .await?,
        )?;
        let tab: o::Id = run.tab_id.context("published tab")?.to_string().parse()?;
        let backend = Arc::new(
            shellfox_remote::Backend::new(
                Arc::new(sender),
                approvals.clone(),
                shellfox_transport::Secret::new(SECRET.into())?,
            )
            .prepare_observer_v2()
            .prepare_manager_v2(),
        );
        let peer = AuthenticatedPeerId::new("n-replay-host")?;
        let observer = ObserverClient::new(
            peer.clone(),
            o::Id::new(),
            SignedFixture(backend.clone(), false),
        );
        observer
            .discover(shellfox_client::observer_v2::FallbackPolicy::Never)
            .await?;
        assert!(matches!(
            observer.pair(o::Id::new()).await?,
            o::Reply::Success(_)
        ));
        let attach = o::AttachTerminal {
            tab_id: tab,
            resume: None,
        };
        let delivery = loop {
            if let o::Reply::Success(o::Attachment::Attached {
                delivery,
                recovery,
                input: (),
                ..
            }) = observer.execute(&attach).await?
            {
                ensure!(
                    String::from_utf8_lossy(&recovery.bytes()?).contains(MARKER),
                    "observer replay lost marker"
                );
                let ack = o::AcknowledgeTerminal {
                    delivery: delivery.clone(),
                    ack: o::Ack::Recovery {
                        recovery_id: recovery.recovery_id,
                        sequence: recovery.last_sequence,
                    },
                };
                assert!(matches!(
                    observer.execute(&ack).await?,
                    o::Reply::Success(_)
                ));
                break delivery;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let o::Reply::Success(catalog) = observer
            .execute(&o::GetSnapshot::from(wire::GetSnapshot {}))
            .await?
        else {
            anyhow::bail!("observer catalog unavailable");
        };
        assert!(catalog.entries.iter().any(|entry| match entry {
            o::CatalogEntry::Session { value } => value.tabs.iter().any(|candidate| {
                candidate.id == tab
                    && matches!(
                        candidate.source_kind.known(),
                        Some(o::SourceKind::ExternalBytes)
                    )
                    && matches!(
                        candidate.facts.lifecycle.known(),
                        Some(wire::TabLifecycle::Closed)
                    )
                    && candidate.facts.exit_code.known() == Some(&Some(0))
            }),
            _ => false,
        }));
        let manager = ManagerClient::new(peer, m::Id::new(), SignedFixture(backend, true));
        manager
            .discover(shellfox_client::manager_v2::FallbackPolicy::Never)
            .await?;
        assert!(matches!(
            manager.pair(m::Id::new()).await?,
            m::Reply::Success(_)
        ));
        let m::Reply::Success(m::Attachment::Attached(replay)) = manager
            .execute(&m::AttachTerminal {
                tab_id: tab.to_string().parse()?,
                resume: None,
                control: m::ControlRequest::IfFree,
            })
            .await?
        else {
            anyhow::bail!("manager replay unavailable");
        };
        ensure!(
            replay.input_state == m::InputState::IntrinsicReadOnly
                && replay.input.is_none()
                && replay.layout.is_none()
        );
        ensure!(String::from_utf8_lossy(&replay.recovery.bytes()?).contains(MARKER));
        let m::Reply::Failure(error) = manager
            .execute(&m::TakeTerminalControl {
                delivery: replay.delivery,
                mode: m::TakeMode::Takeover,
            })
            .await?
        else {
            anyhow::bail!("manager takeover unexpectedly succeeded");
        };
        assert_eq!(error.code, ErrorCode::AccessDenied);
        approvals.revoke(&store, PEER).await.map_err(app)?;
        assert!(!matches!(
            observer.execute(&o::DetachTerminal { delivery }).await,
            Ok(o::Reply::Success(_))
        ));
        assert!(!matches!(
            observer.execute(&attach).await,
            Ok(o::Reply::Success(o::Attachment::Attached { .. }))
        ));
        actor.desktop_unregister(registration.token).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await;
    owner.shutdown().await?;
    result??;
    Ok(())
}
