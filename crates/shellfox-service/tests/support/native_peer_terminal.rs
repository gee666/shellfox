use super::*;
use shellfox_client::manager_v2::ManagerClient;
use shellfox_core::{CreateSession, ProfileId, SessionId};
use shellfox_host::HostRequest;
use shellfox_mobile::ManagerRecovery;
use shellfox_protocol::{OutboundTransport, ProfileEnvironment};

pub async fn exercise<T: OutboundTransport>(
    native: &NativeClient,
    client: Arc<ManagerClient<T>>,
    data: &Path,
) -> Result<ManagerRecovery<T>> {
    let catalog = native.execute(NativeAction::TerminalProfiles).await?;
    let NativeResponse::TerminalProfiles(catalog) = catalog else {
        anyhow::bail!("Actual target catalog absent")
    };
    let profile = catalog
        .profiles
        .profiles
        .iter()
        .find(|p| {
            p.available
                && matches!(p.environment, ProfileEnvironment::Local)
                && catalog.profiles.default_profile_id.as_deref() == Some(p.id.as_str())
        })
        .or_else(|| {
            catalog
                .profiles
                .profiles
                .iter()
                .find(|p| p.available && matches!(p.environment, ProfileEnvironment::Local))
        })
        .context("Actual target local catalog profile unavailable")?;
    let request = SessionId::new();
    let created = native
        .execute(NativeAction::Metadata(HostRequest::CreateSession(
            CreateSession {
                request_id: request,
                title: Some("real isolated peer".into()),
                cwd: data.to_string_lossy().into_owned(),
                profile_id: ProfileId::new(profile.id.clone())?,
            },
        )))
        .await?;
    let NativeResponse::Snapshot(created) = created else {
        anyhow::bail!("Actual target CreateSnapshot absent")
    };
    let shellfox_host::CreationRequestReceipt::SessionCreated { session_id } =
        native.creation_request_receipt(request).await?
    else {
        anyhow::bail!("Actual immutable creation receipt absent")
    };
    let session = created
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .context("Created target session absent")?;
    let tab: m::Id = session
        .tabs
        .first()
        .context("Created target tab absent")?
        .id
        .to_string()
        .parse()?;
    let visible = consumer::reply(
        client
            .execute(&m::GetSnapshot(shellfox_protocol::GetSnapshot {}))
            .await?,
    )?;
    ensure!(
        visible
            .entries
            .iter()
            .any(|entry| entry.id().to_string() == session_id.to_string()),
        "Native and Manager did not share the same Host metadata"
    );
    let mut view = sdk(ManagerRecovery::from_shared_native0(
        client,
        80,
        24,
        shellfox_ghostty_vt::extension::SnapshotLimits::default(),
    ))?;
    let attached = consumer::reply(sdk(view
        .attach(m::AttachTerminal {
            tab_id: tab,
            resume: None,
            control: m::ControlRequest::IfFree,
        })
        .await)?)?;
    let attached = match attached {
        m::Attachment::Attached(attached)=>attached,
        m::Attachment::Unavailable{reason,..}=>anyhow::bail!("Real remote source unavailable: {reason:?}"),
    };
    ensure!(
        attached.input_state == m::InputState::Owned
            && attached.input.is_some()
            && attached.layout.is_some(),
        "Actual Runtime control/layout lease absent"
    );
    ensure!(
        matches!(attached.recovery, m::Recovery::Native0RawV1 { .. }),
        "Actual source did not provide Native0 recovery"
    );
    ensure!(
        view.acknowledge_consumed().await.is_err(),
        "Uninstalled recovery manufactured consumption credit"
    );
    let prepared = sdk(view.prepare_pending())?;
    ensure!(
        view.acknowledge_consumed().await.is_err(),
        "Prepared-only recovery manufactured consumption credit"
    );
    sdk(view.commit_prepared(prepared))?;
    consumer::reply(sdk(view.acknowledge_consumed().await)?)?;
    let command = b"echo SHELLFOX_REAL_TSNET_INPUT\r";
    let pending = sdk(view.admit_input(
        m::Input::Bytes {
            encoding: m::Encoding::Base64,
            data: shellfox_protocol::encode_chunk(command)?,
            origin: m::BytesOrigin::Key,
        },
        m::InputHints {
            user_initiated: true,
            activity_hint: false,
            settled_at: Some(session.settled_at.clone()),
        },
    ))?;
    let accepted = consumer::reply(sdk(view.execute_input(pending).await)?)?;
    ensure!(
        accepted.accepted_bytes == command.len() as u32,
        "Actual PTY receipt did not confirm input bytes"
    );
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        consumer::reply(sdk(view.poll().await)?)?;
        sdk(view.pump())?;
        let screen = sdk(view.owner_view().frame())?;
        let seen = screen.cells.chunks(usize::from(screen.cols)).any(|row| {
            row.iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .trim()
                == "SHELLFOX_REAL_TSNET_INPUT"
        });
        if seen {
            consumer::reply(sdk(view.acknowledge_consumed().await)?)?;
            break;
        }
        let _ = view.acknowledge_consumed().await;
        ensure!(
            Instant::now() < deadline,
            "Real Go/SDK decoded executed output was not observed"
        );
    }
    eprintln!("Real Runtime lease, fresh decode/install, consumed ACK, accepted input and executed output observed over tsnet.");
    Ok(view)
}
