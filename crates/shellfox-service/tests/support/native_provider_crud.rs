//! Same original Service CRUD regression, isolated from the pure/compatibility suite.
use super::*;

async fn step(client: &NativeClient, action: NativeAction, phase: &str) -> Result<NativeResponse> {
    let started = std::time::Instant::now();
    eprintln!("provider-service {phase} begin");
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(20), client.execute(action)).await;
    eprintln!(
        "provider-service {phase} returned after {}ms",
        started.elapsed().as_millis()
    );
    result.map_err(|_| {
        anyhow::anyhow!("cooperative operation budget ended at {phase}; original cleanup requested")
    })?
}
#[test]
fn original_boot_installs_system_and_ssh_owned_store_before_ready() -> Result<()> {
    let started = std::time::Instant::now();
    let mut fixture = support::Fixture::new()?;
    let data = fixture.data("configured-providers");
    let calibration = inputs::resource(fixture.helper.clone(), fixture.digest)?;
    let clock = std::time::Instant::now();
    for _ in 0..3 {
        calibration.validate_current()?;
    }
    eprintln!(
        "provider-service three helper validations {}ms {calibration:?}",
        clock.elapsed().as_millis()
    );
    let (providers, opener) = inputs::providers(&data, &fixture.helper, fixture.digest)?;
    let services = providers.system_services();
    let supervisor = inputs::program("CLI")?;
    let remote = app_composition::NativeApplicationConfig::new(
        &data,
        None,
        Some("owned-providers".into()),
        app_composition::SourceFactory::Local,
    )?
    .remote_options(supervisor.path().to_owned(), false)?
    .with_native_providers(providers)?;
    let ssh_path = data.join("rust-service-v1/ssh");
    ensure!(!ssh_path.exists(), "source planning created SSH namespace");
    let home = data.clone();
    support::cycle_remote(&fixture, &data, remote, |owner, _| async move {
        let client = NativeClient::new(owner);
        let second = client.fork();
        eprintln!(
            "provider-service Ready after {}ms",
            started.elapsed().as_millis()
        );
        ensure!(
            ssh_path.is_dir(),
            "providers were not installed before Ready"
        );
        let NativeResponse::System(SystemNativeResponse::Status(status)) = step(
            &client,
            NativeAction::System(SystemNativeRequest::Status {
                scope: SystemScope::User,
            }),
            "system-status",
        )
        .await?
        else {
            anyhow::bail!("original System provider absent");
        };
        ensure!(
            Path::new(
                &status
                    .facts
                    .home
                    .map_err(|error| anyhow::anyhow!(error.message))?
                    .cwd
            ) == home
        );
        ensure!(
            status.facts.cli.is_err(),
            "fixture claimed a stable installed package"
        );
        // Explicit fixture action through the SAME backend. This opener records
        // targets and never invokes Explorer, a browser, xdg-open or global state.
        services
            .perform_user_action(SystemAction::OpenDirectory { path: home.clone() })
            .map_err(|error| anyhow::anyhow!(error.message))?;
        ensure!(opener.count() == 1);
        let id = uuid::Uuid::new_v4();
        let input = ssh::Input {
            id,
            name: "owned-only".into(),
            host: "fixture.invalid".into(),
            port: 22,
            user: "fixture".into(),
            password: ssh::Password::Clear,
            key_file: None,
            remote_cwd: None,
        };
        let NativeResponse::Ssh(ssh::Response::Profiles(rows)) = step(
            &client,
            NativeAction::Ssh(ssh::Request::Save(input.clone())),
            "save",
        )
        .await?
        else {
            anyhow::bail!("SSH save did not use installed owner");
        };
        ensure!(rows.iter().any(|row| row.id == id));
        let NativeResponse::Ssh(ssh::Response::Profiles(rows)) =
            step(&second, NativeAction::Ssh(ssh::Request::List), "list").await?
        else {
            anyhow::bail!("second native client cannot read same owned store");
        };
        ensure!(rows.iter().any(|row| row.id == id));
        let mut updated = input;
        updated.name = "updated-owned-only".into();
        let NativeResponse::Ssh(ssh::Response::Profiles(rows)) = step(
            &client,
            NativeAction::Ssh(ssh::Request::Save(updated)),
            "update",
        )
        .await?
        else {
            anyhow::bail!("SSH update unavailable");
        };
        ensure!(rows
            .iter()
            .any(|row| row.id == id && row.name == "updated-owned-only"));
        ensure!(
            client
                .execute(NativeAction::Ssh(ssh::Request::Import))
                .await
                .is_err(),
            "unsupported import falsely reported an empty successful scan"
        );
        let NativeResponse::Ssh(ssh::Response::Profiles(rows)) = step(
            &second,
            NativeAction::Ssh(ssh::Request::Delete { id }),
            "delete",
        )
        .await?
        else {
            anyhow::bail!("SSH delete unavailable");
        };
        ensure!(!rows.iter().any(|row| row.id == id));
        eprintln!(
            "provider-service CRUD complete after {}ms; original bootstrap cleanup follows",
            started.elapsed().as_millis()
        );
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
