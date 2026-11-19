use super::*;
pub(super) async fn state(client: &NativeClient) -> Result<RemoteControlState> {
    match client
        .execute(NativeAction::Remote(RemoteRequest::State))
        .await?
    {
        NativeResponse::Remote(state) => Ok(state),
        _ => anyhow::bail!("Actual remote state absent"),
    }
}
pub(super) async fn online(
    client: &NativeClient,
    other: &str,
    expected_control: &str,
) -> Result<AuthenticatedPeerId> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut last = String::new();
    let mut login_requested = false;
    loop {
        let state = state(client).await?;
        let stamp = format!(
            "enabled={} status={:?} peers={} online={} matching={} attempts={} retained={}",
            state.enabled,
            state.status,
            state.peers.len(),
            state.peers.iter().filter(|p| p.online).count(),
            state.peers.iter().filter(|p| p.hostname == other).count(),
            state.attempts.len(),
            state.cleanup_retained
        );
        let stamp = format!(
            "{stamp} health={}",
            state
                .error
                .as_ref()
                .map_or("none", |error| health::message(&error.message))
        );
        if stamp != last {
            eprintln!("Actual Go state awaiting {other}: {stamp}");
            last = stamp;
        }
        if matches!(
            state.status,
            shellfox_service::remote::native::RemoteNetworkStatus::LoginRequired
        ) && !login_requested
        {
            // Explicit real operator login, exactly once, only after the actual
            // producer asks for it. No auth key, env fallback or implicit PAIR.
            login_requested = true;
            if let Some(login) = &state.login_url {
                let actual = url::Url::parse(login)
                    .map_err(|_| anyhow::anyhow!("Invalid actual login URL; value withheld"))?;
                let expected = url::Url::parse(expected_control)?;
                ensure!(
                    actual.origin() == expected.origin(),
                    "Actual login origin differs from explicit private control; URL withheld"
                );
                eprintln!(
                    "Actual login origin matches explicit private control; auth URL withheld."
                );
            }
            client
                .execute(NativeAction::Remote(RemoteRequest::Login {
                    auth_key: None,
                }))
                .await?;
        }
        if matches!(
            state.status,
            shellfox_service::remote::native::RemoteNetworkStatus::Online
        ) {
            if let Some(peer) = state.peers.iter().find(|p| p.online && p.hostname == other) {
                return Ok(AuthenticatedPeerId::new(&peer.id)?);
            }
        }
        ensure!(
            Instant::now() < deadline,
            "Actual Go node/peer did not become online: {last}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
pub(super) fn options(
    data: &Path,
    hostname: &str,
    control: ControlServerOrigin,
) -> Result<shellfox_service::local::RemoteBootOptions> {
    let supervisor = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-test-supervisor"));
    let config = application::NativeApplicationConfig::new(
        data,
        None,
        Some(hostname.into()),
        application::SourceFactory::Native0,
    )?;
    let resources = RemoteBootResources::planned_with_control_server(
        config.remote_state_dir,
        hostname.into(),
        control,
    )?;
    // Network integration uses the base public Service composition. Optional
    // System/SSH installation plans are a separate product acceptance lane.
    Ok(shellfox_service::local::RemoteBootOptions::native(
        supervisor,
        application::system_services(false),
        shellfox_service::system::SystemScope::User,
        resources,
    )?
    .with_observer_factory_policy(config.source_factory.policy()?))
}
pub(super) fn control() -> Result<(ControlServerOrigin, String)> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Ready {
        kind: String,
        control_url: String,
        loopback_only: bool,
        production_account: bool,
        stun_enabled: bool,
    }
    let path = std::fs::canonicalize(PathBuf::from(
        std::env::var_os("SHELLFOX_ACCEPT_CONTROL_READY")
            .context("Original retained fixture ready record required")?,
    ))?;
    let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp"))?;
    ensure!(
        path.starts_with(root),
        "Fixture record must be under project tmp"
    );
    let ready: Ready = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        ready.kind == "OWNED_TESTCONTROL_FIXTURE_NOT_PRODUCTION_AUTH"
            && ready.loopback_only
            && !ready.production_account
            && !ready.stun_enabled,
        "Unsafe fixture configuration"
    );
    Ok((
        ControlServerOrigin::parse_control_url(&ready.control_url)?,
        ready.control_url,
    ))
}
