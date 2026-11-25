use super::*;

/// Proof from the owning bootstrap, never inferred from a thread result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalBootStatus {
    NoOwnerClean,
    RetainedCleanup,
    Ready,
}
#[derive(Clone, Debug)]
pub struct LocalBootReport {
    pub status: LocalBootStatus,
    /// Present at clean completion, even when the operation returned an error.
    /// Dirty metadata after lost ownership is not an unconfirmed live resource.
    pub shutdown: Option<crate::ShutdownOutcome>,
    /// Independent artifact evidence, never process cleanup or serialized authority.
    pub endpoint: Option<std::sync::Arc<crate::NativeEndpointReport>>,
}
/// This attempt acquired no ownership: the original Go contender was waited
/// after reporting an occupied lock. Not constructible from a readiness error.
#[derive(Clone, Debug)]
pub struct OccupiedOwnerError {
    directory: std::path::PathBuf,
    _filesystem: std::sync::Arc<crate::security::NativeCreationRetention>,
}
impl OccupiedOwnerError {
    pub(super) fn proved(
        directory: std::path::PathBuf,
        _proof: shellfox_transport::NoOwnerProof,
        filesystem: std::sync::Arc<crate::security::NativeCreationRetention>,
    ) -> Self {
        Self {
            directory,
            _filesystem: filesystem,
        }
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}
impl std::fmt::Display for OccupiedOwnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Another host owns the isolated service directory.")
    }
}
impl std::error::Error for OccupiedOwnerError {}
/// Concrete owner for the existing embedded local host. Keep it outside the
/// runtime/unwind boundary; run and cleanup only borrow its original resources.
pub struct LocalBootstrap {
    pub(super) boot: boot::BootGuard,
}
/// One-way, local application stop request. Dropping a client/view is not Stop.
#[derive(Clone)]
pub struct LocalBootStop {
    closing: shellfox_protocol::CancellationToken,
    orderly: shellfox_protocol::CancellationToken,
}
impl LocalBootStop {
    pub fn request(&self) {
        self.orderly.cancel();
        self.closing.cancel();
    }
}
impl LocalBootstrap {
    /// `shellfox-cli` flag for the host process that the desktop UI starts.
    pub const DESKTOP_HOST_FLAG: &str = "--desktop-host";
    /// The desktop host's stdout line once its local API admits clients.
    pub const DESKTOP_HOST_READY: &str = "shellfox-desktop-host-ready";
    /// Written by the UI to the desktop host's stdin before it stops the host.
    pub const DESKTOP_HOST_QUIT: u8 = b'q';
    /// Plans/retains the original preparation before any helper or creation.
    pub fn plan(data: &Path, helper: &Path, digest: [u8; 32]) -> Result<Self> {
        preflight_boot_paths(data, helper)?;
        let private = PrivateDirectory::plan(data, helper, digest, true)?;
        Ok(Self {
            boot: boot::BootGuard::new(private)?,
        })
    }
    /// Preserve filesystem evidence and the original typed error after unwind.
    pub fn retain_error(&self, error: anyhow::Error) -> anyhow::Error {
        preparation::retain_error(error, &self.boot.private)
    }
    pub fn stop_handle(&self) -> LocalBootStop {
        LocalBootStop {
            closing: self.boot.server.stop.clone(),
            orderly: self.boot.server.orderly.clone(),
        }
    }
    pub async fn run_with_dispatcher<D: crate::OwnerDispatcher + 'static>(
        &mut self,
        dispatcher: D,
        ready: impl FnOnce(crate::OwnerClient),
        report: impl Fn(LocalBootReport),
    ) -> Result<()> {
        serve_retained_plan(&mut self.boot, dispatcher, None, ready, report).await
    }
    pub async fn run_remote(
        &mut self,
        remote: RemoteBootOptions,
        ready: impl FnOnce(crate::OwnerClient),
        report: impl Fn(LocalBootReport),
    ) -> Result<()> {
        serve_retained_plan(&mut self.boot, (), Some(remote), ready, report).await
    }
    /// Also used after cancellation/unwind of a run future. Does not infer
    /// cleanup from the operation error or from completion of the owning thread.
    pub async fn cleanup(&mut self, report: impl Fn(LocalBootReport)) {
        let report_cb = |value| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| report(value)));
        };
        let progress = |status| {
            if status != LocalBootStatus::NoOwnerClean {
                report_cb(LocalBootReport {
                    status,
                    shutdown: None,
                    endpoint: None,
                });
            }
        };
        let shutdown = self.boot.cleanup(&progress).await;
        report_cb(LocalBootReport {
            status: LocalBootStatus::NoOwnerClean,
            shutdown: Some(shutdown),
            endpoint: Some(self.boot.endpoint_report()),
        });
    }
    /// A foreground host process: serve on this thread until Stop, then retry
    /// cleanup until it is confirmed, also after a panic. `ready` runs once, when
    /// local clients are first admitted.
    pub fn serve_foreground(
        data: &Path,
        helper: &Path,
        digest: [u8; 32],
        remote: RemoteBootOptions,
        runtime: &tokio::runtime::Runtime,
        ready: impl FnOnce(),
    ) -> Result<()> {
        let mut bootstrap = Self::plan(data, helper, digest)?;
        // Ends terminals a previous host of this directory left without an owner.
        #[cfg(target_os = "linux")]
        shellfox_terminal::unix::orphans::adopt(data);
        let clean = std::cell::Cell::new(false);
        let ready = std::cell::Cell::new(Some(ready));
        let report = |report: LocalBootReport| {
            clean.set(report.status == LocalBootStatus::NoOwnerClean);
            if report.status == LocalBootStatus::Ready {
                if let Some(ready) = ready.take() {
                    ready();
                }
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.block_on(bootstrap.run_remote(remote, |_| {}, &report))
        }))
        .unwrap_or_else(|_| {
            Err(bootstrap.retain_error(anyhow::anyhow!(
                "Host execution panicked; original cleanup retained."
            )))
        });
        const CLEANUP_ATTEMPTS: u32 = 5;
        for attempt in 1..=CLEANUP_ATTEMPTS {
            if clean.get() {
                break;
            }
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.block_on(bootstrap.cleanup(&report));
            }))
            .is_err();
            if panicked && attempt < CLEANUP_ATTEMPTS {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        if clean.get() {
            return result;
        }
        let cleanup = anyhow::anyhow!(
            "Host cleanup did not finish after {CLEANUP_ATTEMPTS} attempts; the next start retries it."
        );
        Err(match result {
            Err(error) => error.context(cleanup),
            Ok(()) => cleanup,
        })
    }
}
pub use crate::remote::SourceFactsInstaller;
pub struct RemoteBootOptions {
    /// Actual trusted dispatcher executable; never infer it from a test/GUI host.
    pub supervisor: std::path::PathBuf,
    pub install_source: SourceFactsInstaller,
    pub facts: std::sync::Arc<dyn crate::remote::ManagerFactsProvider>,
    pub resources: crate::remote::RemoteBootResources,
    pub(crate) native_providers: Option<crate::native_providers::NativeHostProviders>,
    system_scope: crate::system::SystemScope,
}
impl RemoteBootOptions {
    /// Rebind facts and native actions to the SAME concrete backend. Planning
    /// does not create SSH storage or scan personal profiles/import sources.
    pub fn with_native_providers(
        mut self,
        providers: crate::native_providers::NativeHostProviders,
    ) -> Result<Self> {
        let projector = crate::activity::integration::NativeSourceFactsProjector::new(
            self.supervisor.clone(),
            providers.system_services(),
            self.system_scope,
        )?;
        let slot = std::sync::Arc::new(crate::remote::SourceFactsProviderSlot::new(
            std::sync::Arc::new(projector),
        ));
        self.facts = slot.clone();
        self.install_source = slot.installer();
        self.native_providers = Some(providers);
        Ok(self)
    }
    /// One composition for live boot and owned fixtures. The system provider is
    /// caller-supplied actual evidence, never guessed installation defaults.
    pub fn native<R: Into<crate::remote::RemoteBootResources>>(
        supervisor: std::path::PathBuf,
        system: std::sync::Arc<crate::system::HostSystemServices>,
        scope: crate::system::SystemScope,
        resources: R,
    ) -> Result<Self> {
        let projector = crate::activity::integration::NativeSourceFactsProjector::new(
            supervisor.clone(),
            system,
            scope,
        )?;
        let slot = std::sync::Arc::new(crate::remote::SourceFactsProviderSlot::new(
            std::sync::Arc::new(projector),
        ));
        Ok(Self {
            supervisor,
            facts: slot.clone(),
            install_source: slot.installer(),
            resources: resources.into(),
            native_providers: None,
            system_scope: scope,
        })
    }
}
/// Compatibility entry: retain and poll the future through cleanup. Arbitrary
/// abort/drop is not cleanup-safe. Product owners use the retained LocalBootstrap.
pub async fn serve(data: &Path, helper: &Path, digest: [u8; 32]) -> Result<()> {
    serve_with_dispatcher(data, helper, digest, ()).await
}
/// Compatibility entry: retain and poll the future through cleanup. Arbitrary
/// abort/drop is not cleanup-safe. Product owners use the retained LocalBootstrap.
pub async fn serve_with_dispatcher<D: crate::OwnerDispatcher + 'static>(
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
    dispatcher: D,
) -> Result<()> {
    serve_with_ready(data, helper, digest, dispatcher, |_| {}).await
}
/// Compatibility entry: retain and poll the future through cleanup. Arbitrary
/// abort/drop is not cleanup-safe. Product owners use the retained LocalBootstrap.
pub async fn serve_with_ready<D: crate::OwnerDispatcher + 'static>(
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
    dispatcher: D,
    ready: impl FnOnce(crate::OwnerClient),
) -> Result<()> {
    serve_with_boot_status(data, helper, digest, dispatcher, ready, |_| {}).await
}
/// Compatibility entry: retain and poll the future through cleanup. Arbitrary
/// abort/drop is not cleanup-safe. Product owners use the retained LocalBootstrap.
pub async fn serve_with_boot_status<D: crate::OwnerDispatcher + 'static>(
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
    dispatcher: D,
    ready: impl FnOnce(crate::OwnerClient),
    status: impl Fn(LocalBootStatus),
) -> Result<()> {
    serve_with_boot_report(data, helper, digest, dispatcher, ready, move |report| {
        status(report.status)
    })
    .await
}
/// Compatibility entry: retain and poll the future through cleanup. Arbitrary
/// abort/drop is not cleanup-safe. Product owners use the retained LocalBootstrap.
pub async fn serve_with_boot_report<D: crate::OwnerDispatcher + 'static>(
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
    dispatcher: D,
    ready: impl FnOnce(crate::OwnerClient),
    report: impl Fn(LocalBootReport),
) -> Result<()> {
    serve_boot_plan(data, helper, digest, dispatcher, None, ready, report).await
}
/// Compatibility entry: retain and poll the future through cleanup. Arbitrary
/// abort/drop is not cleanup-safe. Product owners use the retained LocalBootstrap.
pub async fn serve_remote_with_boot_status(
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
    remote: RemoteBootOptions,
    ready: impl FnOnce(crate::OwnerClient),
    status: impl Fn(LocalBootStatus),
) -> Result<()> {
    serve_remote_with_boot_report(data, helper, digest, remote, ready, move |report| {
        status(report.status)
    })
    .await
}
/// Compatibility entry: retain and poll the future through cleanup. Arbitrary
/// abort/drop is not cleanup-safe. Product owners use the retained LocalBootstrap.
pub async fn serve_remote_with_boot_report(
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
    remote: RemoteBootOptions,
    ready: impl FnOnce(crate::OwnerClient),
    report: impl Fn(LocalBootReport),
) -> Result<()> {
    serve_boot_plan(data, helper, digest, (), Some(remote), ready, report).await
}
