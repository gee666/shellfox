//! Protected readonly discovery over the owner's launch configuration.
#[cfg(test)]
#[path = "native_catalog_tests.rs"]
mod tests;
use crate::*;
use shellfox_core::{ProfileId, Settings};
use shellfox_protocol::{AppError, CancellationToken};
use std::thread::JoinHandle;
use tokio::sync::oneshot;

const MAX_JOBS: usize = 4;
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum NativePythonFact {
    Unknown { reason: String },
    Detected { executable: String },
    NotFound { reason: String },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeLaunchCatalog {
    pub probe: shellfox_protocol::NativeProbe,
    pub profiles: shellfox_protocol::TerminalProfiles,
    pub python: NativePythonFact,
}
#[derive(Default)]
pub(crate) struct NativeCatalogState {
    jobs: Vec<Job>,
    #[cfg(windows)]
    pub(crate) wsl: crate::windows_profiles::WslCatalogWorker,
}
struct Job {
    controller: u64,
    settings: Settings,
    default_profile: ProfileId,
    cancel: CancellationToken,
    thread: Option<JoinHandle<std::result::Result<NativeLaunchCatalog, AppError>>>,
    reply: Option<CatalogReply>,
}
enum CatalogReply {
    Native(oneshot::Sender<Result<crate::native::NativeResponse>>),
    Desktop(oneshot::Sender<Result<serde_json::Value>>),
}
impl CatalogReply {
    fn is_closed(&self) -> bool {
        match self { Self::Native(reply) => reply.is_closed(), Self::Desktop(reply) => reply.is_closed() }
    }
    fn send(self, result: Result<crate::native::NativeResponse>) {
        match self {
            Self::Native(reply) => { let _ = reply.send(result); }
            Self::Desktop(reply) => {
                let result = result.and_then(crate::local::DesktopReply::try_from).and_then(|response| Ok(serde_json::to_value(response)?));
                let _ = reply.send(result);
            }
        }
    }
}
impl Service {
    pub(crate) fn enqueue_native_catalog(&mut self, controller: u64, reply: oneshot::Sender<Result<crate::native::NativeResponse>>) {
        self.enqueue_catalog(controller, CatalogReply::Native(reply));
    }

    pub(crate) fn enqueue_desktop_catalog(&mut self, controller: u64, reply: oneshot::Sender<Result<serde_json::Value>>) {
        self.enqueue_catalog(controller, CatalogReply::Desktop(reply));
    }

    fn enqueue_catalog(&mut self, controller: u64, reply: CatalogReply) {
        let result = (|| -> Result<Job> {
            self.assert_accepting()?;
            if controller == 0 || self.catalog.jobs.len() >= MAX_JOBS {
                bail!("Native catalog discovery is unavailable or busy; refresh explicitly.");
            }
            let settings = self.host.global_settings()?;
            let default_profile = self.host.snapshot()?.preferences.default_profile_id;
            let discovery =
                crate::activity_integration::NativeFactsDiscovery::new(self.supervisor.clone())?;
            let cancel = self.ownership_lost.child_token();
            let input_settings = settings.clone();
            let input_default = default_profile.clone();
            let input_cancel = cancel.clone();
            #[cfg(windows)]
            let wsl_cache = self.catalog.wsl.started_cache();
            let thread = std::thread::Builder::new()
                .name("shellfox-native-catalog".into())
                .spawn(move || {
                    #[cfg(windows)]
                    let wsl = wsl_cache.and_then(|cache| cache.wait(&input_cancel));
                    let facts = discovery.discover_catalog(
                        &input_settings,
                        &input_default,
                        &input_cancel,
                        #[cfg(windows)]
                        wsl.as_deref(),
                    )?;
                    Ok(NativeLaunchCatalog {
                        probe: facts.probe,
                        profiles: facts.profiles,
                        python: NativePythonFact::Unknown {
                            reason: "No owner-validated Python detection was performed.".into(),
                        },
                    })
                })?;
            Ok(Job {
                controller,
                settings,
                default_profile,
                cancel,
                thread: Some(thread),
                reply: None,
            })
        })();
        match result {
            Ok(mut job) => {
                job.reply = Some(reply);
                self.catalog.jobs.push(job);
            }
            Err(error) => reply.send(Err(error)),
        }
    }
    pub(crate) fn pump_native_catalog(&mut self) {
        #[cfg(windows)]
        if self.catalog.wsl.poll() { self.activity.dirty = true; }
        let jobs = std::mem::take(&mut self.catalog.jobs);
        for mut job in jobs {
            if self.stopping || job.reply.as_ref().is_none_or(|reply| reply.is_closed()) {
                job.cancel.cancel();
            }
            if job
                .thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
            {
                self.catalog.jobs.push(job);
                continue;
            }
            let result=job.thread.take().context("Native discovery owner is missing.").and_then(|thread|thread.join().map_err(|_|anyhow::anyhow!("Native catalog worker stopped."))).and_then(|result|result.map_err(|error|anyhow::anyhow!(error.message))).and_then(|catalog| {
                self.assert_accepting()?;
                if job.cancel.is_cancelled()||self.host.global_settings()?!=job.settings||self.host.snapshot()?.preferences.default_profile_id!=job.default_profile {bail!("Owner catalog configuration changed or authority ended; refresh explicitly.");}
                Ok(crate::native::NativeResponse::TerminalProfiles(Box::new(catalog)))
            });
            if let Some(reply) = job.reply.take() {
                reply.send(result);
            }
        }
    }
    pub(crate) fn revoke_native_catalog(&mut self, controller: u64) {
        for job in self
            .catalog
            .jobs
            .iter()
            .filter(|job| job.controller == controller)
        {
            job.cancel.cancel();
        }
    }
    pub(crate) fn begin_stop_native_catalog(&mut self) {
        #[cfg(windows)]
        self.catalog.wsl.cancel();
        for job in &self.catalog.jobs {
            job.cancel.cancel();
        }
    }
    pub(crate) fn stop_native_catalog(&mut self) -> Result<()> {
        self.begin_stop_native_catalog();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            self.pump_native_catalog();
            #[cfg(windows)]
            let wsl_running = self.catalog.wsl.running();
            #[cfg(not(windows))]
            let wsl_running = false;
            if self.catalog.jobs.is_empty() && !wsl_running {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("Original native catalog discovery worker is still running; owner retained.");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
