//! Private public-API targets. Execute only with original suspended/pidfd custody.
use shellfox_service::app_composition;
#[path = "support/native_provider_crud.rs"]
mod crud;
#[path = "support/native_provider_inputs.rs"]
mod inputs;
#[path = "support/native_provider_lease_tests.rs"]
mod lease_tests;
#[path = "support/native_bootstrap_remote_owned.rs"]
mod support;
use anyhow::{ensure, Result};
use shellfox_service::{
    native::{NativeAction, NativeClient, NativeResponse},
    ssh,
    system::*,
};
use std::path::Path;

#[test]
fn producer_verified_read_returns_exact_bounded_bytes_and_retains_its_guard() -> Result<()> {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root)?;
    let fixture = tempfile::Builder::new()
        .prefix("verified-provider-read-")
        .tempdir_in(root)?;
    let file = fixture.path().join("producer-input");
    std::fs::write(&file, b"provider-program-fixture\n")?;
    let gone = Arc::new(AtomicBool::new(false));
    let helper = shellfox_transport::VerifiedHelper::new(
        file.clone(),
        inputs::digest_file(&file)?,
    )?
    .with_resource_guard(Arc::new(Guard(gone.clone())));
    ensure!(helper.read_verified(1024)? == b"provider-program-fixture\n");
    ensure!(helper.read_verified(1).is_err());
    ensure!(helper.read_verified(0).is_err());
    ensure!(!gone.load(Ordering::SeqCst));
    std::fs::write(&file, b"changed resource")?;
    ensure!(helper.read_verified(1024).is_err());
    std::fs::remove_file(&file)?;
    ensure!(helper.read_verified(1024).is_err());
    drop(helper);
    ensure!(gone.load(Ordering::SeqCst));
    Ok(())
}
#[test]
fn compatibility_local_boot_stays_available_without_provider_resources() -> Result<()> {
    let fixture = support::Fixture::new()?;
    let data = fixture.data("compatibility-no-providers");
    support::cycle(&fixture, &data, |owner, _| async move {
        let client = NativeClient::new(owner);
        ensure!(
            matches!(client.execute(NativeAction::Status).await?, NativeResponse::Status(status) if status.active_terminals == 0)
        );
        Ok(())
    })?;
    Ok(())
}
#[test]
fn query_only_startup_needs_no_ssh_executables() -> Result<()> {
    let fixture = support::Fixture::new()?;
    let data = fixture.data("query-only-default");
    let supervisor = inputs::program()?;
    let mut arguments = app_composition::ApplicationArguments::default();
    arguments.push("--remote-hostname", "owned-query-only".into())?;
    let config = arguments.finish(&data)?;
    let remote = config.remote_options(supervisor.path().to_owned(), false)?;
    let ssh_path = data.join("rust-service-v1/ssh");
    ensure!(!ssh_path.exists(), "planning created SSH storage");
    support::cycle_remote(&fixture, &data, remote, |owner, _| async move {
        let client = NativeClient::new(owner);
        let NativeResponse::System(SystemNativeResponse::Status(status)) = client
            .execute(NativeAction::System(SystemNativeRequest::Status {
                scope: SystemScope::User,
            }))
            .await?
        else {
            anyhow::bail!("System status unavailable after real readiness");
        };
        ensure!(
            status.facts.cli.is_err(),
            "development CLI was presented as installed package evidence"
        );
        ensure!(matches!(
            status.capabilities.cli.install,
            SystemCapability::Unavailable(_)
        ));
        ensure!(matches!(
            status.capabilities.cli.uninstall,
            SystemCapability::Unavailable(_)
        ));
        ensure!(matches!(
            status.capabilities.explorer.install,
            SystemCapability::Unavailable(_)
        ));
        ensure!(matches!(
            status.capabilities.explorer.uninstall,
            SystemCapability::Unavailable(_)
        ));
        ensure!(client
            .execute(NativeAction::System(SystemNativeRequest::SetCli {
                scope: SystemScope::User,
                installed: true
            }))
            .await
            .is_err());
        ensure!(ssh_path.is_dir(), "SSH settings should work without OpenSSH");
        ensure!(matches!(client.execute(NativeAction::Ssh(ssh::Request::List)).await?,
            NativeResponse::Ssh(ssh::Response::Profiles(rows)) if rows.is_empty()));
        Ok(())
    })?;
    Ok(())
}
