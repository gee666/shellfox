//! Missing own-fixture key fails before keyscan/network; repeated owner polls
//! must preserve the failure after actual worker cleanup.
use super::*;
use std::time::{Duration, Instant};
#[test]
fn preparation_failure_remains_failed_through_repeated_owner_polls() -> Result<()> {
    let mut fixture = support::Fixture::new()?;
    let data = fixture.data("failed-preparation");
    let (providers, _opener) = inputs::providers(&data, &fixture.helper, fixture.digest)?;
    let cli = inputs::program("CLI")?;
    let options = app_composition::NativeApplicationConfig::new(
        &data,
        None,
        Some("owned-preparation-failure".into()),
        app_composition::SourceFactory::Local,
    )?
    .remote_options(cli.path().to_owned(), false)?
    .with_native_providers(providers)?;
    let missing = data.join("missing-native-test-key");
    ensure!(!missing.exists());
    support::cycle_remote(&fixture, &data, options, |owner, _| async move {
        let client = NativeClient::new(owner);
        let profile = uuid::Uuid::new_v4();
        client
            .execute(NativeAction::Ssh(ssh::Request::Save(ssh::Input {
                id: profile,
                name: "owned-missing-key".into(),
                host: "192.168.0.107".into(),
                port: 22,
                user: "oira666".into(),
                password: ssh::Password::Clear,
                key_file: Some(missing.to_string_lossy().into_owned()),
                remote_cwd: Some("/home/oira666/shellfox-ghostti-migration.8lza8G".into()),
            })))
            .await?;
        let NativeResponse::SshOpened(opened) = client
            .execute(NativeAction::SshOpen(ssh::OpenProfile {
                request_id: shellfox_core::SessionId::new(),
                profile,
                cols: 80,
                rows: 24,
            }))
            .await?
        else {
            anyhow::bail!("open receipt missing")
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut failure = None;
        for _ in 0..5 {
            loop {
                let NativeResponse::Ssh(ssh::Response::Status(rows)) = client
                    .execute(NativeAction::Ssh(ssh::Request::GetStatus {
                        connection: Some(opened.connection.connection),
                    }))
                    .await?
                else {
                    anyhow::bail!("status missing")
                };
                let row = rows
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("original receipt missing"))?;
                match &row.state {
                    ssh::State::Failed { message } => {
                        ensure!(row.runtime.is_none());
                        if let Some(old) = &failure {
                            ensure!(old == message, "original failure changed after cleanup");
                        } else {
                            failure = Some(message.clone());
                        }
                        break;
                    }
                    ssh::State::Closed => {
                        anyhow::bail!("cleanup erased the original preparation failure")
                    }
                    _ => {
                        ensure!(Instant::now() < deadline, "preparation failure deadline");
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        ensure!(failure.is_some());
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
