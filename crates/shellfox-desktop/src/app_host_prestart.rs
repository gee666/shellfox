//! Typed endpoint boot before graphical initialization. No release/reacquire.
use super::{AppHost, EndpointBackend};
use crate::host_client::{HostEndpoint, HostEndpointConfig};
use anyhow::Result;
use shellfox_service::local::OccupiedOwnerError;
use std::sync::Arc;
#[cfg(test)]
#[path = "app_host_prestart_tests.rs"]
mod tests;

pub enum AppHostPrestart {
    Ready(AppHost),
    ReadyLocalOnly {
        host: AppHost,
        remote_unavailable: shellfox_service::remote::RemoteUnavailableReason,
    },
    /// Only authorizes attempting authenticated bounded handoff to that directory.
    /// It says nothing about a later launch-owner fence or desktop availability.
    Occupied(OccupiedOwnerError),
    Failed {
        host: AppHost,
        error: Arc<anyhow::Error>,
    },
}
impl<B: super::AppHostBackend> AppHost<B> {
    /// Roll back a retained pre-GPU owner after GPU/init failure. Registry state
    /// only proves no window is using it, never that resource ownership is zero.
    pub async fn cleanup_prestarted(&self) -> Result<Option<shellfox_service::ShutdownOutcome>> {
        let _operation = self.shared.operation.lock().await;
        {
            let mut state = self.state()?;
            anyhow::ensure!(
                state.windows.is_empty() && state.pending.is_none(),
                "Prestart cleanup cannot remove registered or pending windows"
            );
            if state.stopped {
                return Ok(state.shutdown_outcome.clone());
            }
            // Also fences a cancelled wait; a later call resumes the SAME Stop.
            state.cleanup_retained = true;
            state.revision += 1;
        }
        let outcome = self.shared.backend.stop().await?;
        let mut state = self.state()?;
        state.shutdown_outcome = outcome.clone(); // Retain opaque evidence BEFORE stopped.
        state.stopped = true;
        state.cleanup_retained = false;
        state.revision += 1;
        Ok(outcome)
    }
}
impl AppHost {
    pub async fn prestart(config: HostEndpointConfig) -> Result<AppHostPrestart> {
        Self::prestart_with_dispatcher(config, ()).await
    }
    pub async fn prestart_with_dispatcher<D: shellfox_service::OwnerDispatcher + 'static>(
        config: HostEndpointConfig,
        dispatcher: D,
    ) -> Result<AppHostPrestart> {
        // start's only outer errors occur before its thread exists. Once spawned,
        // EVERY error retains this exact endpoint and its owning cleanup thread.
        Self::await_prestarted_endpoint(HostEndpoint::start_with_dispatcher(config, dispatcher)?)
            .await
    }
    pub async fn prestart_remote(
        config: HostEndpointConfig,
        remote: shellfox_service::local::RemoteBootOptions,
    ) -> Result<AppHostPrestart> {
        Self::await_prestarted_endpoint(HostEndpoint::start_remote(config, remote)?).await
    }
    async fn await_prestarted_endpoint(mut endpoint: HostEndpoint) -> Result<AppHostPrestart> {
        match endpoint.ready().await {
            Ok(_) => {
                let outcome = endpoint.startup_outcome();
                let host = Self::with_backend(EndpointBackend::from_endpoint(endpoint));
                Ok(match outcome {
                    crate::host_client::HostEndpointStartupOutcome::Ready => {
                        AppHostPrestart::Ready(host)
                    }
                    crate::host_client::HostEndpointStartupOutcome::ReadyLocalOnly {
                        remote_unavailable,
                    } => AppHostPrestart::ReadyLocalOnly {
                        host,
                        remote_unavailable,
                    },
                    crate::host_client::HostEndpointStartupOutcome::NoOwnerClean
                    | crate::host_client::HostEndpointStartupOutcome::RetainedCleanup => {
                        AppHostPrestart::Failed {
                            host,
                            error: Arc::new(anyhow::anyhow!(
                                "Startup client arrived without actual local readiness"
                            )),
                        }
                    }
                })
            }
            Err(error) => {
                if let Some(proof) = endpoint.occupied_owner() {
                    // This branch is sealed Go acquire(None)+contender-exit proof,
                    // not a NoOwner bool, text match or unreadable endpoint guess.
                    return Ok(AppHostPrestart::Occupied(proof));
                }
                let error = endpoint
                    .startup_failure()
                    .unwrap_or_else(|| Arc::new(error));
                Ok(AppHostPrestart::Failed {
                    host: Self::with_backend(EndpointBackend::from_endpoint(endpoint)),
                    error,
                })
            }
        }
    }
}
