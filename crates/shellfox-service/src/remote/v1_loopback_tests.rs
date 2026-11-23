use super::*;
use crate::{activity_integration::NativeSourceFactsProjector, owner::ServiceOwner, system::*};
use anyhow::{Context, Result};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct NoInstall;
impl SystemInstallResources for NoInstall {
    fn cli_plan(&self, _: SystemScope) -> SystemResult<CliInstallPlan> {
        Err(no_install())
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        Err(no_install())
    }
    fn validate_current(&self) -> SystemResult<()> {
        Err(no_install())
    }
}
fn no_install() -> AppError {
    failure(
        ErrorCode::Unsupported,
        "Fixture does not install application resources.",
    )
}
fn app<T>(result: std::result::Result<T, AppError>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("{:?}: {}", error.code, error.message))
}

#[tokio::test]
#[ignore = "requires the compiled Shellfox CLI supervisor and bundled mobile TS loopback client"]
async fn mobile_v1_client_drives_the_real_rust_owner() -> Result<()> {
    let executable =
        PathBuf::from(std::env::var_os("SHELLFOX_TEST_SUPERVISOR").context("compiled CLI path")?);
    let script =
        std::env::var_os("SHELLFOX_TEST_MOBILE_CLIENT").context("bundled mobile TS client")?;
    let data = crate::tests::scratch();
    let files = crate::tests::scratch();
    let file_root =
        shellfox_transport::normalize_private_path(&std::fs::canonicalize(files.path())?)?;
    std::fs::write(file_root.join("loopback.txt"), "owned loopback fixture")?;
    let fixture = crate::native_metadata_fixture::Fixture::new();
    let system = Arc::new(HostSystemServices::new(Arc::new(
        NativeSystemBackend::native(Arc::new(NoInstall), Default::default(), false),
    )));
    let slot = Arc::new(SourceFactsProviderSlot::new(Arc::new(
        NativeSourceFactsProjector::new(executable.clone(), system, SystemScope::User)?,
    )));
    let (sender, receiver) = remote_channel();
    let mut state = RemoteState::new(Default::default(), None, None);
    state.transfers = Some(app(shellfox_remote::RemoteTransfers::open(Default::default()))?);
    let dispatcher = RemoteDispatcher::new(receiver, state)
        .with_facts_provider(slot.clone(), tokio::runtime::Handle::current());
    let mut owner = ServiceOwner::start(
        data.path().to_owned(),
        executable,
        fixture.lost.clone(),
        dispatcher,
    )
    .await?;
    let result = async {
        let actor = owner.client(uuid::Uuid::new_v4());
        let store = shellfox_remote::PrivateApprovalStore::retained_native(
            fixture.files.clone(),
            fixture.lost.clone(),
        );
        app(store.bind_repository(actor.approval_repository(fixture.lost.clone())))?;
        app(store.initialize_read_only())?;
        let control = app(shellfox_remote::ApprovalControl::load(&store).await)?;
        app(control.set_enabled(true))?;
        owner.activate().await?;
        slot.install(actor.source_facts_watch().await?)?;
        let secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let backend = Arc::new(shellfox_remote::Backend::new(
            Arc::new(sender),
            control.clone(),
            shellfox_transport::Secret::new(secret.clone())?,
        ));
        let (_, server) = backend.bind_until(fixture.lost.clone()).await?;
        let result = async {
            let mut child = tokio::process::Command::new("node")
                .arg(script)
                .env("SHELLFOX_TEST_ADDR", server.address().to_string())
                .env("SHELLFOX_TEST_SECRET", secret)
                .env("SHELLFOX_TEST_CWD", &file_root)
                .kill_on_drop(true)
                .spawn()?;
            let timeout = tokio::time::timeout(Duration::from_secs(35), async {
                let mut approved = false;
                let mut revoked = false;
                loop {
                    if let Some(status) = child.try_wait()? {
                        anyhow::ensure!(
                            status.success() && approved && revoked,
                            "Mobile loopback client failed: {status}"
                        );
                        return Ok::<_, anyhow::Error>(());
                    }
                    if !approved
                        && app(control.state())?
                            .pending
                            .iter()
                            .any(|pending| pending.peer.id == "n-mobile-loopback")
                    {
                        app(control
                            .approve_full_account(&store, "n-mobile-loopback")
                            .await)?;
                        approved = true;
                    }
                    if approved && !revoked && files.path().join("revoke-device").is_file() {
                        app(control.revoke(&store, "n-mobile-loopback").await)?;
                        revoked = true;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await;
            timeout?
        }
        .await;
        server.shutdown().await;
        result
    }
    .await;
    owner.shutdown().await?;
    result
}
