//! Private public-API targets. Execute only with original suspended/pidfd custody.
#[path = "../../native_app_composition.rs"]
mod app_composition;
#[path = "support/native_provider_crud.rs"]
mod crud;
#[path = "support/native_provider_ssh.rs"]
mod ssh_live;
#[path = "support/native_provider_failure.rs"]
mod preparation_failure;
#[path = "support/native_provider_permission_contract.rs"]
mod permission_contract;
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
fn bounded_direct_store_diagnoses_permission_verifier_cost_without_a_host() -> Result<()> {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let started = Instant::now();
    let mark = |phase: &str| {
        eprintln!(
            "provider-store-diagnostic {}ms {phase}",
            started.elapsed().as_millis()
        )
    };
    let mut fixture = support::Fixture::new()?;
    mark("private fixture ready, no Host or network started");
    let helper = inputs::resource(fixture.helper.clone(), fixture.digest)?;
    mark("actual pinned helper lease verified");
    for _ in 0..3 {
        helper.validate_current()?;
    }
    mark("three extra helper validations complete");
    let policy = if cfg!(windows) {
        shellfox_ssh::StoragePolicy::with_windows_helper(helper.path())
    } else {
        shellfox_ssh::StoragePolicy::default()
    };
    let directory = Arc::new(shellfox_ssh::PrivateDirectory::open(
        fixture.data("direct-store"),
        policy,
    )?);
    let store = shellfox_ssh::ProfileStore::new(directory.clone());
    mark("private store open, existing raw permissions policy");
    let id = uuid::Uuid::new_v4();
    let input = shellfox_ssh::ProfileInput {
        id,
        name: "owned-diagnostic".into(),
        host: "fixture.invalid".into(),
        port: 22,
        user: "fixture".into(),
        password: shellfox_ssh::PasswordUpdate::Clear,
        key_file: None,
        remote_cwd: None,
    };
    let operation = Instant::now();
    ensure!(store.save(input)?.iter().any(|p| p.id == id));
    mark("save returned");
    ensure!(
        operation.elapsed() < Duration::from_secs(20),
        "save exceeded cooperative operation budget"
    );
    let operation = Instant::now();
    ensure!(store.list()?.iter().any(|p| p.id == id));
    mark("list returned");
    ensure!(
        operation.elapsed() < Duration::from_secs(20),
        "list exceeded cooperative operation budget"
    );
    let operation = Instant::now();
    ensure!(!store.delete(id)?.iter().any(|p| p.id == id));
    mark("delete returned");
    ensure!(
        operation.elapsed() < Duration::from_secs(20),
        "delete exceeded cooperative operation budget"
    );
    drop(store);
    drop(directory);
    drop(helper);
    mark("all store and program descriptors closed");
    fixture.success();
    Ok(())
}
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
    let fixture = tempfile::Builder::new()
        .prefix("verified-provider-read-")
        .tempdir_in(root)?;
    let file = fixture.path().join("producer-input");
    std::fs::write(&file, b"provider-program-fixture\n")?;
    let gone = Arc::new(AtomicBool::new(false));
    let helper = shellfox_transport::VerifiedHelper::new(
        file.clone(),
        inputs::digest("593e72c0b7aabeff5d43bc242b289b5602127a83a1f7404e74045ad61cc542c2")?,
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
    let mut fixture = support::Fixture::new()?;
    let data = fixture.data("compatibility-no-providers");
    support::cycle(&fixture, &data, |owner, _| async move {
        let client = NativeClient::new(owner);
        ensure!(
            matches!(client.execute(NativeAction::Status).await?, NativeResponse::Status(status) if status.active_terminals == 0)
        );
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
#[test]
fn default_query_only_startup_reaches_ready_without_stable_package_or_ssh_storage() -> Result<()> {
    let mut fixture = support::Fixture::new()?;
    let data = fixture.data("query-only-default");
    let supervisor = inputs::program("CLI")?;
    let mut arguments = app_composition::ApplicationArguments::default();
    arguments.push("--remote-hostname", "owned-query-only".into())?;
    let config = arguments.finish_with_helper(&data, &fixture.helper, fixture.digest)?;
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
        ensure!(
            !ssh_path.exists(),
            "query-only startup materialized SSH storage"
        );
        ensure!(
            client
                .execute(NativeAction::Ssh(ssh::Request::List))
                .await
                .is_err(),
            "unconfigured SSH was falsely installed"
        );
        Ok(())
    })?;
    fixture.success();
    Ok(())
}
