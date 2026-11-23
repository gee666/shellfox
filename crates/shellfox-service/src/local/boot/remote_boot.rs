use super::*;
#[derive(Debug)]
pub(super) struct RemoteInitializationFailure(pub(super) shellfox_protocol::AppError);
impl std::fmt::Display for RemoteInitializationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Remote setup failed: {}", self.0.message)
    }
}
impl std::error::Error for RemoteInitializationFailure {}

impl BootGuard {
    pub(crate) async fn start_remote(
        &mut self,
        options: crate::local::RemoteBootOptions,
    ) -> Result<()> {
        anyhow::ensure!(self.remote.is_none(), "Remote envelope is already retained");
        anyhow::ensure!(
            self.actor.is_none()
                && self.supervisor.is_none()
                && options.supervisor.is_absolute()
                && options.supervisor.is_file(),
            "Original explicit supervisor is unavailable before actor creation"
        );
        self.providers.plan = options.native_providers;
        self.supervisor = Some(options.supervisor);
        let control = self
            .prepare_remote_envelope(options.facts, options.resources)
            .await?;
        self.remote_control = Some(control);
        self.install_source = Some(options.install_source);
        let actor = self
            .actor
            .as_ref()
            .context("Original actor missing before remote install")?;
        self.remote
            .as_ref()
            .context("Original remote envelope missing")?
            .install_dispatcher(actor, &self.server.stop)
            .await?;
        self.publish_activate().await
    }
    pub(super) async fn prepare_remote_envelope(
        &mut self,
        facts: std::sync::Arc<dyn crate::remote::ManagerFactsProvider>,
        resources: impl Into<crate::remote::RemoteBootResources>,
    ) -> Result<crate::remote::RemoteControl> {
        anyhow::ensure!(self.remote.is_none(), "Remote envelope is already retained");
        self.server.ensure_running()?;
        self.control_files()?;
        let ownership = self.preparation.acquired_view()?;
        anyhow::ensure!(ownership.is_alive(), "Native host ownership lost.");
        self.private
            .prepare_owned_layout(ownership.permission_verifier())
            .await?;
        self.server.ensure_running()?;
        self.control_files
            .as_mut()
            .context("Native control owner missing")?
            .bind(&self.private)?;
        let control_child = self.native_control_child()?;
        // Existing Go library state/config below is still path-based. This
        // retained Rust capability does not anchor or approve those Go paths.
        self.remote = Some(crate::remote::RemoteHostSetup::retained(
            control_child,
            &ownership,
        ));
        // Approval persistence uses the original actor before repository RPC.
        self.start_actor(()).await?;
        // The exact envelope owns all partial resources BEFORE borrowed IO.
        let acquired = self.preparation.acquired_view()?;
        let control = self
            .remote
            .as_mut()
            .context("Remote envelope missing")?
            .initialize(
                &self.private.helper,
                facts,
                resources.into(),
                &self.private.path,
                acquired.permission_verifier(),
            )
            .await
            .map_err(|error| anyhow::Error::new(RemoteInitializationFailure(error)))?;
        Ok(control)
    }
}
