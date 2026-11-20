use super::*;
impl ServiceOwner {
    pub(super) fn open_for_actor(
        directory: &std::path::Path,
        supervisor: &std::path::Path,
        lost: &CancellationToken,
        lifetime: Option<shellfox_transport::RootLifetimeGuard>,
    ) -> std::result::Result<Service, StartupFailure> {
        #[cfg(windows)]
        let start_wsl_discovery = lifetime.is_some();
        let opened = match lifetime {
            Some(lifetime) => {
                Service::open_owned_with_root_lifetime(directory, supervisor, lifetime)
            }
            None => Service::open_owned(directory, supervisor),
        };
        let mut service = opened.map_err(|cause| StartupFailure {
            cause,
            shutdown: cleanup::committed(),
        })?;
        service.ownership_lost = lost.clone();
        #[cfg(windows)]
        if start_wsl_discovery { service.catalog.wsl.start(lost); }
        let runtime = crate::settings_runtime::SettingsRuntimeAdapter::new(lost.clone());
        #[cfg(windows)]
        let runtime = runtime.with_wsl_catalog(service.catalog.wsl.cache.clone());
        if let Err(error) = service
            .host()
            .install_settings_runtime(std::sync::Arc::new(runtime))
        {
            return Err(StartupFailure {
                cause: error.into(),
                shutdown: cleanup::finish(service),
            });
        }
        Ok(service)
    }
    /// Install the real controller in the caller's retained slot before waiting.
    pub async fn start_with_root_lifetime<D: OwnerDispatcher + Send + 'static>(
        slot: &mut Option<Self>,
        directory: PathBuf,
        supervisor: PathBuf,
        lost: CancellationToken,
        lifetime: shellfox_transport::RootLifetimeGuard,
        dispatcher: D,
    ) -> Result<()> {
        anyhow::ensure!(
            slot.is_none(),
            "Original service owner is already installed"
        );
        *slot = Some(Self::spawn_inner(
            directory,
            supervisor,
            lost,
            Some(lifetime),
            dispatcher,
        )?);
        let owner = slot.as_mut().context("Original service owner missing")?;
        if let Err(error) = owner.wait_started().await {
            let shutdown = loop {
                match owner.shutdown().await {
                    Ok(outcome) => break outcome,
                    Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            };
            return Err(StartupFailure {
                cause: error,
                shutdown,
            }
            .into());
        }
        Ok(())
    }
    pub(super) async fn wait_started(&mut self) -> Result<()> {
        let result = self
            .started
            .as_mut()
            .context("Service startup reply already consumed")?
            .await
            .context("Service owner failed before startup.");
        self.started = None;
        result?
    }
    #[cfg(test)]
    pub async fn start<D: OwnerDispatcher + Send + 'static>(
        directory: PathBuf,
        supervisor: PathBuf,
        lost: CancellationToken,
        dispatcher: D,
    ) -> Result<Self> {
        let mut owner = Self::spawn_inner(directory, supervisor, lost, None, dispatcher)?;
        if let Err(error) = owner.wait_started().await {
            let shutdown = loop {
                match owner.shutdown().await {
                    Ok(outcome) => break outcome,
                    Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            };
            return Err(StartupFailure {
                cause: error,
                shutdown,
            }
            .into());
        }
        Ok(owner)
    }
}
