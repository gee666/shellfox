use anyhow::{Context, Result};
use shellfox_service::{
    local::{LocalBootReport, LocalBootStatus, LocalBootStop, LocalBootstrap},
    OwnerClient,
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub struct Fixture {
    pub helper: PathBuf,
    pub digest: [u8; 32],
    success: bool,
    plan: shellfox_files::DirectoryPlan,
    _scope: Arc<shellfox_files::FileService>,
    keeper: shellfox_files::DirectoryPreparationOwner,
    // Last: original namespace pins close before scratch deletion.
    scratch: Option<tempfile::TempDir>,
}
impl Fixture {
    pub fn new() -> Result<Self> {
        let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp"))?;
        #[cfg(windows)]
        let root = PathBuf::from(
            root.to_str()
                .context("fixture UTF8")?
                .strip_prefix(r"\\?\")
                .unwrap_or(root.to_str().context("fixture UTF8")?),
        );
        let scratch = tempfile::Builder::new()
            .prefix("public-native-owned-")
            .tempdir_in(root)?;
        let authority = Arc::new(shellfox_files::FileService::approved_root(
            scratch.path(),
            shellfox_files::Access::ReadWrite,
        )?);
        let keeper = authority.directory_preparation_owner();
        let plan = authority.plan_directory(&shellfox_files::Location {
            path: "verified-hosts".into(),
        })?;
        let scope = plan
            .create_private_approved_root()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        plan.revalidate()?;
        keeper.verify_retained()?;
        let helper = PathBuf::from(
            std::env::var_os("SHELLFOX_ACCEPT_HELPER").context("current owned helper required")?,
        );
        let hex = std::env::var("SHELLFOX_ACCEPT_HELPER_SHA256")?;
        anyhow::ensure!(hex.len() == 64, "helper digest length");
        let mut digest = [0; 32];
        for (i, b) in digest.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
        }
        Ok(Self {
            helper,
            digest,
            success: false,
            plan,
            _scope: scope,
            keeper,
            scratch: Some(scratch),
        })
    }
    pub fn data(&self, name: &str) -> PathBuf {
        self.plan.configured_path().join(name)
    }
    pub fn success(&mut self) {
        self.success = true;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.keeper.preserve_stale() {
            eprintln!("Namespace disposition retained: {error}");
            self.success = false;
        }
        if !self.success {
            if let Some(dir) = self.scratch.take() {
                eprintln!(
                    "Owned acceptance fixture retained: {}",
                    dir.keep().display()
                );
            }
        }
    }
}
#[derive(Clone, Default)]
pub struct Reports(Arc<Mutex<Vec<LocalBootReport>>>);
impl Reports {
    pub fn send(&self, value: LocalBootReport) {
        self.0.lock().expect("report lock").push(value);
    }
    pub fn cleanup_started_after_ready(&self) -> bool {
        let reports = self.0.lock().expect("report lock");
        reports
            .iter()
            .position(|r| {
                matches!(
                    r.status,
                    LocalBootStatus::Ready | LocalBootStatus::ReadyLocalOnly { .. }
                )
            })
            .is_some_and(|ready| {
                reports
                    .iter()
                    .skip(ready + 1)
                    .any(|r| r.status == LocalBootStatus::RetainedCleanup)
            })
    }
    pub fn clean(&self) -> Result<()> {
        let reports = self.0.lock().map_err(|_| anyhow::anyhow!("report lock"))?;
        anyhow::ensure!(
            reports
                .last()
                .is_some_and(|r| r.status == LocalBootStatus::NoOwnerClean && r.shutdown.is_some()),
            "actual no-owner report absent"
        );
        Ok(())
    }
}
pub struct StopOnDrop(pub LocalBootStop);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.request();
    }
}
pub fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

pub(super) fn cycle_inner<T, F, Work>(
    fixture: &Fixture,
    data: &Path,
    remote: Option<shellfox_service::local::RemoteBootOptions>,
    driver: F,
) -> Result<T>
where
    F: FnOnce(OwnerClient, LocalBootStop) -> Work,
    Work: Future<Output = Result<T>>,
{
    let runtime = runtime()?;
    let mut boot = LocalBootstrap::plan(data, &fixture.helper, fixture.digest)?;
    let stop = boot.stop_handle();
    let reports = Reports::default();
    let callback = reports.clone();
    let operation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            let (ready, received) = tokio::sync::oneshot::channel();
            let run = async {
                let ready = move |owner| {
                    let _ = ready.send(owner);
                };
                let report = move |r| callback.send(r);
                match remote {
                    Some(remote) => boot.run_remote(remote, ready, report).await,
                    None => boot.run_with_dispatcher((), ready, report).await,
                }
            };
            let drive = async {
                let _stop = StopOnDrop(stop.clone());
                let client = received.await.context("real local readiness absent")?;
                driver(client, stop).await
            };
            let (served, driven) = tokio::join!(run, drive);
            served?;
            driven
        })
    }));
    let result = match operation {
        Ok(value) => value,
        Err(_) => Err(boot.retain_error(anyhow::anyhow!("acceptance driver unwind"))),
    };
    // SAME installed bootstrap even after a cancelled/unwound run, never rediscovery.
    if reports.clean().is_err() {
        runtime.block_on(boot.cleanup(|r| reports.send(r)));
    }
    reports.clean()?;
    if result.is_ok() {
        anyhow::ensure!(
            reports.cleanup_started_after_ready(),
            "successful public run skipped retained cleanup progress"
        );
    }
    result
}
