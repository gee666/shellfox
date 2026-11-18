//! One retained manager. Only the blocking worker touches its mutex; the owner
//! reads confirmed reports and joins finished workers without waiting on OS calls.
use super::{contract::bounded, PowerBackend, PowerManager, PowerReport, PowerStatus, SystemPower};
use shellfox_protocol::CancellationToken;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub(crate) struct PowerWorker<B: PowerBackend = SystemPower> {
    manager: Arc<Mutex<PowerManager<B>>>,
    job: Option<JoinHandle<PowerReport>>,
    report: PowerReport,
    stop: CancellationToken,
    worker_lost: bool,
}
impl Default for PowerWorker {
    fn default() -> Self {
        Self::new(SystemPower)
    }
}
impl<B: PowerBackend + 'static> PowerWorker<B>
where
    B::Lease: 'static,
{
    pub(crate) fn new(backend: B) -> Self {
        Self {
            manager: Arc::new(Mutex::new(PowerManager::with_backend(backend))),
            job: None,
            report: PowerReport {
                status: PowerStatus::Inactive,
                partial_reason: None,
                error: None,
                pending: false,
            },
            stop: CancellationToken::new(),
            worker_lost: false,
        }
    }
    pub(crate) fn report(&self) -> PowerReport {
        self.report.clone()
    }
    pub(crate) fn uncertain(&self) -> bool {
        self.worker_lost
    }
    /// The Service calls this at receipt admission with its ORIGINAL ownership
    /// token, including when loss raced the worker's final acquisition check.
    pub(crate) fn admit_owner_receipt(
        &mut self,
        lost: &CancellationToken,
    ) -> anyhow::Result<PowerReport> {
        if !lost.is_cancelled() {
            return Ok(self.report());
        }
        self.begin_stop();
        if !self.report.pending && self.report.status != PowerStatus::Inactive {
            self.start(false, CancellationToken::new(), lost.clone())?;
        }
        anyhow::bail!("Original host ownership lost before keep-awake receipt; cleanup retained.");
    }
    pub(crate) fn begin_stop(&self) {
        self.stop.cancel();
    }
    pub(crate) fn start(
        &mut self,
        active: bool,
        cancel: CancellationToken,
        lost: CancellationToken,
    ) -> anyhow::Result<()> {
        if self.job.is_some() || self.worker_lost {
            anyhow::bail!("Keep-awake operation is pending or its worker is unconfirmed.");
        }
        if active && self.stop.is_cancelled() {
            anyhow::bail!("Host is stopping; keep-awake enable was not admitted.");
        }
        let manager = self.manager.clone();
        let stop = self.stop.clone();
        let (registered, wait_registered) = std::sync::mpsc::sync_channel(1);
        let job = std::thread::Builder::new()
            .name("shellfox-power".into())
            .spawn(move || {
                // Register the original JoinHandle before any OS effect.
                if wait_registered.recv().is_err() {
                    return PowerReport {
                        status: PowerStatus::Inactive,
                        partial_reason: None,
                        error: Some("Power worker registration lost.".into()),
                        pending: true,
                    };
                }
                let poisoned = manager.is_poisoned();
                let mut manager = manager.lock().unwrap_or_else(|error| error.into_inner());
                let was_inactive = manager.status() == PowerStatus::Inactive;
                let operation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if poisoned {
                        panic!("Original keep-awake manager mutex is poisoned");
                    }
                    if active
                        && (cancel.is_cancelled() || lost.is_cancelled() || stop.is_cancelled())
                    {
                        return Err(super::PowerError(
                            "Keep-awake enable canceled before acquisition.".into(),
                        ));
                    }
                    let result = manager.set(active);
                    // Shutdown can race an OS acquisition. Do not publish that
                    // enable; release the same lease before returning a receipt.
                    if active
                        && (stop.is_cancelled()
                            || lost.is_cancelled()
                            || (was_inactive && cancel.is_cancelled()))
                    {
                        manager.set(false)
                    } else {
                        result
                    }
                }));
                let uncertain = operation.is_err();
                let error = match operation {
                    Ok(result) => result.err().map(|error| bounded(error.to_string())),
                    Err(_) => Some("Keep-awake worker panicked; cleanup must be retried.".into()),
                };
                PowerReport {
                    status: manager.status(),
                    partial_reason: manager.partial_reason().map(bounded),
                    error,
                    pending: uncertain,
                }
            })?;
        self.job = Some(job);
        let _ = registered.send(());
        self.report.pending = true;
        Ok(())
    }
    pub(crate) fn poll(&mut self) {
        if self.job.as_ref().is_none_or(|job| !job.is_finished()) {
            return;
        }
        if let Some(job) = self.job.take() {
            match job.join() {
                Ok(report) => {
                    self.worker_lost = report.pending;
                    self.report = report;
                }
                Err(_) => {
                    self.worker_lost = true;
                    self.report.error =
                        Some("Original keep-awake worker lost; cleanup unconfirmed.".into());
                    self.report.pending = true;
                }
            }
        }
    }
    /// Nonblocking cleanup poll. A failure keeps the manager, lease and job.
    /// Existing Service shutdown supervision retries this same owner.
    pub(crate) fn stop(&mut self) -> anyhow::Result<()> {
        self.begin_stop();
        self.poll();
        if self.report.pending {
            anyhow::bail!("Keep-awake cleanup pending; original owner retained.");
        }
        if self.report.status == PowerStatus::Inactive && self.report.error.is_none() {
            return Ok(());
        }
        if let Some(error) = &self.report.error {
            // Keep the failed report visible until the retry actually completes.
            let error = error.clone();
            self.start(false, CancellationToken::new(), CancellationToken::new())?;
            anyhow::bail!("Keep-awake cleanup retry pending after: {error}");
        }
        self.start(false, CancellationToken::new(), CancellationToken::new())?;
        anyhow::bail!("Keep-awake release pending; original owner retained.");
    }
}
