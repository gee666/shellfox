use super::*;
use std::time::Duration;

async fn step(
    client: &shellfox_service::OwnerClient,
    action: NativeAction,
    phase: &str,
) -> Result<NativeResponse> {
    tokio::time::timeout(Duration::from_secs(20), client.native(action))
        .await
        .map_err(|_| anyhow::anyhow!("{phase} timed out"))?
}

#[test]
fn original_boot_installs_system_and_ssh_owned_store_before_ready() -> Result<()> {
    let fixture = support::Fixture::new()?;
    let data = fixture.data("configured-providers");
    let (providers, opener) = inputs::providers(&data);
    let services = providers.system_services();
    let supervisor = inputs::program()?;
    let remote =
        app_composition::NativeApplicationConfig::new(&data, None, Some("owned-providers".into()))?
            .remote_options(supervisor.path().to_owned(), false)?
            .with_native_providers(providers)?;
    let ssh_path = data.join("rust-service-v1/ssh");
    ensure!(!ssh_path.exists(), "source planning created SSH namespace");
    let home = data.clone();
    let helper = fixture.helper.clone();
    let digest = fixture.digest;
    support::cycle_remote(&fixture, &data, remote, |client, _| async move {
        let second = client.fork();
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
            password: ssh::Password::set("isolated-cli-fixture".into())?,
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
        use shellfox_service::local::{LocalClient, SshRead, SshReadReply};
        let local = LocalClient::connect(&home, &helper, digest).await?;
        let profiles = local.request(&SshRead::List {}).await?;
        ensure!(!serde_json::to_string(&profiles)?.contains("isolated-cli-fixture"));
        ensure!(
            matches!(profiles, SshReadReply::Profiles(rows) if rows.iter().any(|row| row.id == id && row.has_password))
        );
        let SshReadReply::Connection(Some(profile)) =
            local.request(&SshRead::Connection { id }).await?
        else {
            anyhow::bail!("CLI could not read its selected connection");
        };
        ensure!(profile
            .password
            .as_ref()
            .is_some_and(|secret| secret.expose() == "isolated-cli-fixture"));
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
        Ok(())
    })?;
    Ok(())
}
