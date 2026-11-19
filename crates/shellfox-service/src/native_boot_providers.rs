//! Retained provider startup on the original acquired bootstrap and actor.
use super::*;
use crate::{native_providers::NativeHostProviders, ssh::Store};
use std::thread::JoinHandle;

#[derive(Default)]
pub(super) struct NativeProviderState {
    pub plan: Option<NativeHostProviders>,
    system_installed: bool,
    ssh_installed: bool,
    job: Option<JoinHandle<Result<Option<Store>>>>,
    panicked: bool,
}
impl NativeProviderState {
    fn finished(&mut self) -> Result<Option<Option<Store>>> {
        anyhow::ensure!(
            !self.panicked,
            "Original native provider worker panicked; cleanup remains unconfirmed."
        );
        if self.job.as_ref().is_none_or(|job| !job.is_finished()) {
            return Ok(None);
        }
        match self
            .job
            .take()
            .context("Native provider worker missing")?
            .join()
        {
            Ok(result) => result.map(Some),
            Err(_) => {
                self.panicked = true;
                anyhow::bail!("Original native provider worker panicked; bootstrap retained.")
            }
        }
    }
}
impl BootGuard {
    pub(super) async fn install_native_providers(&mut self) -> Result<()> {
        self.server.ensure_running()?;
        let actor = self
            .actor
            .as_ref()
            .context("Original actor missing before provider installation")?;
        let client = actor.client(uuid::Uuid::new_v4());
        if !self.providers.system_installed {
            if let Some(plan) = &self.providers.plan {
                client
                    .install_system_native(plan.system_backend().clone())
                    .await?;
            }
            self.providers.system_installed = true;
        }
        if self.providers.ssh_installed {
            return Ok(());
        }
        if self.providers.job.is_none() {
            let ownership = self.preparation.acquired_view()?;
            anyhow::ensure!(
                ownership.is_alive(),
                "Original native owner lost before SSH preparation"
            );
            let lost = ownership.lost();
            let stop = self.server.stop.clone();
            let location = self.private.ssh_store.clone();
            let (registered, wait_registered) = std::sync::mpsc::sync_channel(1);
            let job = std::thread::Builder::new()
                .name("shellfox-native-providers".into())
                .spawn(move || {
                    wait_registered
                        .recv()
                        .context("Native provider worker registration lost")?;
                    anyhow::ensure!(
                        !lost.is_cancelled() && !stop.load(std::sync::atomic::Ordering::Acquire),
                        "Native provider preparation canceled"
                    );
                    let install = match location.install() {
                        Ok(store) => Some(store),
                        Err(error) => {
                            eprintln!("SSH profile storage unavailable: {error:#}");
                            None
                        }
                    };
                    anyhow::ensure!(
                        !lost.is_cancelled() && !stop.load(std::sync::atomic::Ordering::Acquire),
                        "Native provider authority ended during preparation"
                    );
                    Ok(install)
                })?;
            self.providers.job = Some(job); // Retain original handle BEFORE effects.
            registered
                .send(())
                .context("Native provider worker stopped before registration")?;
        }
        let install = loop {
            self.server.ensure_running()?;
            if let Some(install) = self.providers.finished()? {
                break install;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        self.server.ensure_running()?;
        anyhow::ensure!(
            self.preparation.acquired_view()?.is_alive(),
            "Native ownership lost before SSH installation"
        );
        if let Some(install) = install {
            client.install_ssh_profiles(install).await?;
        }
        self.providers.ssh_installed = true;
        Ok(())
    }
    /// No pending JoinHandle is dropped on canceled or failed startup. Normal
    /// preparation failures leave only private artifacts; actor cleanup is separate.
    pub(super) fn drain_native_provider_worker(&mut self) -> Result<()> {
        if self.providers.panicked {
            anyhow::bail!("Original native provider worker panicked; cleanup unconfirmed.");
        }
        if self
            .providers
            .job
            .as_ref()
            .is_some_and(|job| !job.is_finished())
        {
            anyhow::bail!(
                "Original native provider preparation still running; bootstrap retained."
            );
        }
        if self.providers.job.is_some() {
            // Keep the worker until its file and permissions operations finish.
            let _ = self.providers.finished()?;
        }
        Ok(())
    }
}
