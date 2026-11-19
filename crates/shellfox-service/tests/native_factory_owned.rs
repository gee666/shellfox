//! Actual production composition and first catalog PTY. No GPUI/test supervisor.
#[path = "../../native_app_composition.rs"]
pub mod application;
#[path = "support/native_bootstrap_remote_owned.rs"]
pub mod support;
use anyhow::{ensure, Context, Result};
use shellfox_core::{CreateSession, ProfileId, SessionId};
use shellfox_host::HostRequest;
use shellfox_protocol::{ProfileEnvironment, Validate};
use shellfox_service::native::{
    NativeAction, NativeClient, NativeCommand, NativeInputContext, NativeResponse,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
fn actual_catalog_local_first_pty_uses_explicit_cli_and_stops_original_owner() -> Result<()> {
    first_pty(application::SourceFactory::Local)
}
#[cfg(feature = "experimental-shellfox-native")]
#[test]
fn actual_catalog_native0_first_pty_uses_explicit_cli_and_stops_original_owner() -> Result<()> {
    first_pty(application::SourceFactory::Native0)
}
fn first_pty(factory: application::SourceFactory) -> Result<()> {
    let fixture = support::Fixture::new()?;
    let data = fixture.data(match factory {
        application::SourceFactory::Local => "actual-local-factory",
        application::SourceFactory::Native0 => "actual-native0-factory",
    });
    let supervisor = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-test-supervisor"));
    let config = application::NativeApplicationConfig::new(
        &data,
        None,
        Some("actual-owned-factory".into()),
        factory,
    )?;
    let remote = config.remote_options(supervisor, false)?;
    let cwd = data.to_string_lossy().into_owned();
    // cycle_remote retains the SAME bootstrap and confirms actual Stop/drain/
    // NoOwner before returning any driver error, including an Attach failure.
    support::cycle_remote(&fixture, &data, remote, |owner, stop| async move {
        let client = NativeClient::new(owner);
        let state = client
            .execute(NativeAction::Remote(
                shellfox_service::remote::RemoteRequest::State,
            ))
            .await?;
        let NativeResponse::Remote(state) = state else {
            anyhow::bail!("Actual composed remote control absent")
        };
        ensure!(
            !state.enabled,
            "Acceptance must not start tailnet networking"
        );
        let catalog = client.execute(NativeAction::TerminalProfiles).await?;
        let NativeResponse::TerminalProfiles(catalog) = catalog else {
            anyhow::bail!("Actual owner catalog absent")
        };
        catalog
            .profiles
            .validate()
            .map_err(|_| anyhow::anyhow!("Invalid actual catalog"))?;
        let profiles = &catalog.profiles;
        let selected = profiles
            .profiles
            .iter()
            .find(|p| {
                p.available
                    && matches!(p.environment, ProfileEnvironment::Local)
                    && profiles.default_profile_id.as_deref() == Some(p.id.as_str())
            })
            .or_else(|| {
                profiles
                    .profiles
                    .iter()
                    .find(|p| p.available && matches!(p.environment, ProfileEnvironment::Local))
            })
            .context("No actual available local catalog profile")?;
        let request_id = SessionId::new();
        let snapshot = client
            .execute(NativeAction::Metadata(HostRequest::CreateSession(
                CreateSession {
                    request_id,
                    title: Some("actual first PTY".into()),
                    cwd,
                    profile_id: ProfileId::new(selected.id.clone())?,
                },
            )))
            .await?;
        let NativeResponse::Snapshot(snapshot) = snapshot else {
            anyhow::bail!("CreateSnapshot absent")
        };
        let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } =
            client.creation_request_receipt(request_id).await?
        else {
            anyhow::bail!("Actual creation receipt unavailable")
        };
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .context("Created session absent")?;
        let tab = session.tabs.first().context("Created tab absent")?.id;
        let input = NativeInputContext {
            captured_settlement: shellfox_host::CapturedSettlement::Captured(
                session.settled_at.clone(),
            ),
            user_initiated: true,
            hint: false,
        };
        let attached = client
            .execute(NativeAction::Attach {
                tab,
                takeover: false,
            })
            .await
            .context("Actual first catalog PTY Attach failed")?;
        let NativeResponse::Attached(attachment) = attached else {
            anyhow::bail!("Actual attachment absent")
        };
        client
            .execute(NativeAction::Terminal {
                tab,
                attachment: attachment.clone(),
                command: NativeCommand::Paste("echo SHELLFOX_ACTUAL_FIRST_PTY".into()),
                input: Some(input.clone()),
            })
            .await?;
        client
            .execute(NativeAction::Terminal {
                tab,
                attachment: attachment.clone(),
                command: NativeCommand::Key(shellfox_ghostty_vt::KeyEvent {
                    key: shellfox_ghostty_vt::Key::Enter,
                    ..Default::default()
                }),
                input: Some(input),
            })
            .await?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut seen = false;
        let mut credited = false;
        while Instant::now() < deadline && !(seen && credited) {
            if let NativeResponse::Frame(Some(frame)) = client
                .execute(NativeAction::Frame {
                    tab,
                    attachment: attachment.clone(),
                })
                .await?
            {
                seen |= frame
                    .frame
                    .screen
                    .cells
                    .chunks(usize::from(frame.frame.screen.cols))
                    .any(|row| {
                        row.iter()
                            .map(|c| c.text.as_str())
                            .collect::<String>()
                            .trim()
                            == "SHELLFOX_ACTUAL_FIRST_PTY"
                    });
                let accepted = client
                    .execute(NativeAction::AcceptFrame {
                        tab,
                        attachment: attachment.clone(),
                        frame_id: frame.frame_id,
                    })
                    .await?;
                if let NativeResponse::Accepted { sequence } = accepted {
                    credited |= sequence > 0;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        ensure!(
            seen && credited,
            "Actual executed output and original frame credit were not observed"
        );
        eprintln!("Actual first catalog PTY: executed marker and original frame credit observed.");
        stop.request();
        Ok(())
    })?;
    eprintln!("Actual original Stop/drain and NoOwnerClean confirmed.");
    Ok(())
}

#[test]
fn outgoing_native_and_private_clients_share_disabled_host_without_implicit_pair() -> Result<()> {
    use shellfox_protocol::{AuthenticatedPeerId, CancellationToken, RequestLimits, TransportError};
    use shellfox_service::remote::OutgoingV2Endpoint;
    let fixture = support::Fixture::new()?;
    let data = fixture.data("outgoing-disabled");
    let local_data = data.clone();
    let helper = fixture.helper.clone();
    let digest = fixture.digest;
    let supervisor = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-test-supervisor"));
    let remote = application::NativeApplicationConfig::new(&data, None, Some("owned-outgoing".into()), application::SourceFactory::Local)?
        .remote_options(supervisor, false)?;
    support::cycle_remote(&fixture, &data, remote, |owner, _| async move {
        let native = NativeClient::new(owner);
        let local = shellfox_service::local::LocalClient::connect(&local_data, &helper, digest).await?;
        let peer = AuthenticatedPeerId::new("n-peer")?;
        let limits = || RequestLimits {
            deadline: Instant::now() + Duration::from_secs(5), cancel: CancellationToken::new(),
            max_request_bytes: shellfox_protocol::MAX_REQUEST_BYTES,
            max_response_bytes: shellfox_protocol::MAX_RESPONSE_BYTES,
        };
        for endpoint in [OutgoingV2Endpoint::Discover, OutgoingV2Endpoint::Pair, OutgoingV2Endpoint::Request, OutgoingV2Endpoint::Events, OutgoingV2Endpoint::Disconnect] {
            assert_eq!(native.outgoing_v2_with_status(&peer, endpoint, b"{}".to_vec(), limits()).await, Err(TransportError::Unavailable));
            assert_eq!(local.outgoing_v2_with_status(&peer, endpoint, b"{}".to_vec(), limits()).await, Err(TransportError::Unavailable));
        }
        let cancelled = limits(); cancelled.cancel.cancel();
        assert_eq!(native.outgoing_v2_with_status(&peer, OutgoingV2Endpoint::Request, b"{}".to_vec(), cancelled.clone()).await, Err(TransportError::Cancelled));
        assert_eq!(local.outgoing_v2_with_status(&peer, OutgoingV2Endpoint::Request, b"{}".to_vec(), cancelled).await, Err(TransportError::Cancelled));
        let expired = RequestLimits { deadline:Instant::now()-Duration::from_millis(1), ..limits() };
        assert_eq!(native.outgoing_v2_with_status(&peer, OutgoingV2Endpoint::Request, b"{}".to_vec(), expired.clone()).await, Err(TransportError::Deadline));
        assert_eq!(local.outgoing_v2_with_status(&peer, OutgoingV2Endpoint::Request, b"{}".to_vec(), expired).await, Err(TransportError::Deadline));
        let state = native.execute(NativeAction::Remote(shellfox_service::remote::RemoteRequest::State)).await?;
        let NativeResponse::Remote(state) = state else { anyhow::bail!("Remote state absent") };
        ensure!(!state.enabled && state.attempts.is_empty() && state.pending.is_empty() && state.approvals.is_empty(), "Opaque outgoing operation implicitly changed authority/networking");
        local.disconnect().await?;
        assert_eq!(local.outgoing_v2_with_status(&peer, OutgoingV2Endpoint::Discover, b"{}".to_vec(), limits()).await, Err(TransportError::Unavailable));
        Ok(())
    })?;
    Ok(())
}
