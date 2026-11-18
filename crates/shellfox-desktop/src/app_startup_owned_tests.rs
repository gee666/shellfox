//! Real app-composition entrypoint acceptance. Explicit owned helper/artifact only.
use super::*;
use crate::{
    app_host::{CloseDecision, CloseTarget},
    host_client::HostEndpointStartupOutcome,
    options::app_composition::{NativeApplicationConfig, SourceFactory},
};
use shellfox_files::{Access, DirectoryPlan, DirectoryPreparationOwner, FileService, Location};
use shellfox_service::native::{NativeAction, NativeResponse};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    root: PathBuf,
    plan: DirectoryPlan,
    keeper: DirectoryPreparationOwner,
    _issuer: Arc<FileService>,
    _private: Arc<FileService>,
    success: bool,
}
impl Fixture {
    fn new() -> Result<Self> {
        let base = std::env::current_dir()?.join("tmp");
        let root = PathBuf::from(
            std::env::var_os("SHELLFOX_APP_ACCEPT_SCOPE")
                .context("explicit fresh owned scope required")?,
        );
        anyhow::ensure!(
            root.is_absolute() && root.starts_with(&base) && !root.exists(),
            "fresh project tmp scope required"
        );
        std::fs::create_dir(&root)?;
        let issuer = Arc::new(FileService::approved_root(&root, Access::ReadWrite)?);
        let keeper = issuer.directory_preparation_owner();
        let plan = issuer.plan_directory(&Location {
            path: "private".into(),
        })?;
        let private = match plan.create_private_approved_root() {
            Ok(private) => private,
            Err(mut failure) => {
                failure.preserve_stale()?;
                anyhow::bail!(
                    "owned fixture private preparation failed: {}",
                    failure.error()
                );
            }
        };
        Ok(Self {
            root,
            plan,
            keeper,
            _issuer: issuer,
            _private: private,
            success: false,
        })
    }
    fn data(&self) -> PathBuf {
        self.plan.configured_path().join("host")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.keeper.preserve_stale().is_err() {
            self.success = false;
        }
        // Acceptance artifacts are deliberately retained, never cleanup-by-name on failure.
        if !self.success {
            eprintln!(
                "Application acceptance scope retained: {}",
                self.root.display()
            );
        }
    }
}
fn helper() -> Result<(PathBuf, [u8; 32])> {
    let path = PathBuf::from(
        std::env::var_os("SHELLFOX_ACCEPT_HELPER").context("trusted helper path required")?,
    );
    let hex = std::env::var("SHELLFOX_ACCEPT_HELPER_SHA256")?;
    anyhow::ensure!(hex.len() == 64, "trusted helper SHA256 required");
    let mut digest = [0; 32];
    for (i, b) in digest.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    Ok((path, digest))
}
fn close_original(host: &AppHost, runtime: &tokio::runtime::Runtime) {
    while !host.is_stopped().unwrap_or(false) {
        let result = runtime.block_on(async {
            if host.window_count()? == 0 {
                host.cleanup_prestarted().await?;
                return Ok::<_, anyhow::Error>(());
            }
            let count = host.owned_terminal_count().await?;
            let ticket = match host.request_close(CloseTarget::Quit, count)? {
                CloseDecision::Ready(ticket) => ticket,
                CloseDecision::Confirm(confirmation) => confirmation.approve(),
                CloseDecision::Blocked(_) => {
                    anyhow::bail!("owned acceptance window unexpectedly guarded")
                }
            };
            host.finish_close(ticket).await?;
            Ok(())
        });
        if result.is_err() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
#[test]
#[ignore = "explicit owned helper/supervisor/scope; actual Go/PTY, no accounts or GUI"]
fn real_native_remote_composition_occupied_new_source_and_original_stop() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let (helper, helper_sha256) = helper()?;
    let supervisor = PathBuf::from(
        std::env::var_os("SHELLFOX_APP_TEST_SUPERVISOR")
            .context("actual private product supervisor required")?,
    );
    anyhow::ensure!(
        supervisor.is_absolute() && supervisor.is_file(),
        "private supervisor executable required"
    );
    let factory = SourceFactory::parse(&std::env::var("SHELLFOX_APP_SOURCE_FACTORY")?)?;
    let application = NativeApplicationConfig::new(
        &fixture.data(),
        None,
        Some("owned-app-composition".into()),
        factory,
    )?;
    let config = HostEndpointConfig {
        data_dir: fixture.data(),
        helper,
        helper_sha256,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let remote = application.remote_options(supervisor.clone(), false)?;
    let first = runtime.block_on(AppHost::prestart_remote(config.clone(), remote))?;
    let (host, local_only) = match first {
        AppHostPrestart::Ready(host) => (host, false),
        AppHostPrestart::ReadyLocalOnly { host, .. } => (host, true),
        AppHostPrestart::Failed { host, error } => {
            close_original(&host, &runtime);
            anyhow::bail!("native app startup failed: {error}");
        }
        AppHostPrestart::Occupied(_) => anyhow::bail!("fresh owned app namespace was occupied"),
    };
    let retained = host.clone();
    let work = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            anyhow::ensure!(host.same_owner(&retained), "same app owner required");
            let remote = application.remote_options(supervisor, false)?;
            let AppHostPrestart::Occupied(_) = AppHost::prestart_remote(config, remote).await?
            else {
                anyhow::bail!("competing app must return sealed occupied proof");
            };
            if local_only {
                anyhow::ensure!(
                    !application.remote_state_dir.exists(),
                    "static LocalOnly must precede optional node state creation"
                );
            }
            let window = host.open_window().await?;
            let NativeResponse::TerminalProfiles(catalog) = window
                .client
                .execute(NativeAction::TerminalProfiles)
                .await?
            else {
                anyhow::bail!("actual catalog required");
            };
            let profile = if cfg!(windows) { "cmd" } else { "login-shell" };
            anyhow::ensure!(
                catalog
                    .profiles
                    .profiles
                    .iter()
                    .any(|p| p.id == profile && p.available && p.elevation.is_none()),
                "native fixture profile unavailable"
            );
            let request = shellfox_core::SessionId::new();
            let NativeResponse::Snapshot(snapshot) = window
                .client
                .execute(NativeAction::Metadata(
                    shellfox_host::HostRequest::CreateSession(shellfox_host::CreateSession {
                        request_id: request,
                        title: Some("owned application composition".into()),
                        cwd: fixture
                            .plan
                            .configured_path()
                            .to_str()
                            .context("fixture cwd UTF8")?
                            .into(),
                        profile_id: shellfox_host::ProfileId::new(profile)?,
                    }),
                ))
                .await?
            else {
                anyhow::bail!("actual session snapshot required");
            };
            anyhow::ensure!(
                snapshot.sessions.len() == 1,
                "fresh owned fixture must return exactly one created session"
            );
            let session = snapshot
                .sessions
                .first()
                .context("created session missing")?;
            anyhow::ensure!(
                snapshot.selection.session_id == Some(session.id),
                "create reply must identify its actual new session, not the request UUID"
            );
            let tab = session.tabs.first().context("created tab missing")?.id;
            let NativeResponse::Attached(attachment) = window
                .client
                .execute(NativeAction::Attach {
                    tab,
                    takeover: false,
                })
                .await?
            else {
                anyhow::bail!("actual runtime attachment required");
            };
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let NativeResponse::Frame(Some(frame)) = window
                    .client
                    .execute(NativeAction::Frame {
                        tab,
                        attachment: attachment.clone(),
                    })
                    .await?
                {
                    if let Some(error) = frame.error {
                        anyhow::bail!("new source failed: {error}");
                    }
                    if frame.phase == shellfox_runtime::Phase::Active {
                        window
                            .client
                            .execute(NativeAction::AcceptFrame {
                                tab,
                                attachment: attachment.clone(),
                                frame_id: frame.frame_id,
                            })
                            .await?;
                        break;
                    }
                }
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "new configured source did not reach Active"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            anyhow::ensure!(
                host.owned_terminal_count().await? == 1,
                "one actual native runtime owner required"
            );
            Ok::<_, anyhow::Error>(())
        })
    }));
    // SAME owner and runtime remain outside work/unwind; no LocalClient/fresh discovery Stop.
    close_original(&host, &runtime);
    anyhow::ensure!(
        retained.is_stopped()?
            && runtime.block_on(host.startup_outcome())?
                == HostEndpointStartupOutcome::NoOwnerClean,
        "original stop required"
    );
    eprintln!(
        "Original application owner Stop confirmed through NoOwnerClean; same owner retained."
    );
    match work {
        Ok(result) => result?,
        Err(_) => anyhow::bail!("owned application work panicked after confirmed original Stop"),
    };
    fixture.success = true;
    std::fs::write(
        fixture.root.join("application-result.txt"),
        format!(
            "same-owner=true\nremote-local-only={local_only}\nsource-factory={factory:?}\nnew-program=Active\noriginal-stop=confirmed\n"
        ),
    )?;
    Ok(())
}
