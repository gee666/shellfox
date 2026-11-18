//! Public production route. Run only through the original-handle owned runner.
#[path = "support/native_bootstrap_owned.rs"]
mod support;
use anyhow::{ensure, Result};
use shellfox_service::{local::LocalClient, native::NativeClient, power::PowerStatus};

#[test]
fn local_clients_and_native_windows_share_power_and_shutdown_releases() -> Result<()> {
    let mut fixture = support::Fixture::new()?;
    let data = fixture.data("shared-power");
    let helper = fixture.helper.clone();
    let digest = fixture.digest;
    let client_data = data.clone();
    support::cycle(&fixture, &data, |owner, stop| async move {
        let first = NativeClient::new(owner);
        let second = first.fork();
        let local = LocalClient::connect(&client_data, &helper, digest).await?;
        let other_local = LocalClient::connect(&client_data, &helper, digest).await?;
        ensure!(first.power_status().await?.status == PowerStatus::Inactive);
        ensure!(local.power_status().await?.status == PowerStatus::Inactive);
        let enabled = first.set_power(true).await?;
        ensure!(
            !enabled.pending && enabled.error.is_none(),
            "acquire failed: {enabled:?}"
        );
        ensure!(matches!(
            enabled.status,
            PowerStatus::Active | PowerStatus::Partial
        ));
        ensure!(second.power_status().await? == enabled);
        ensure!(local.power_status().await? == enabled);
        ensure!(other_local.set_power(true).await? == enabled);
        let disabled = local.set_power(false).await?;
        ensure!(disabled.status == PowerStatus::Inactive && disabled.error.is_none());
        ensure!(second.power_status().await? == disabled);
        ensure!(other_local.power_status().await? == disabled);
        let enabled = second.set_power(true).await?;
        ensure!(enabled.error.is_none() && !enabled.pending);
        // Leave the lease enabled. The SAME retained bootstrap must release it
        // before cycle's original NoOwnerClean acknowledgement can succeed.
        stop.request();
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
