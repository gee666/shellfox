//! One protected discovery snapshot per owner. Catalog readers never start guests.
use super::WslProfiles;
use shellfox_protocol::CancellationToken;
use std::{
    sync::{Arc, OnceLock},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Default)]
pub(crate) struct WslCatalogCache(Arc<OnceLock<Option<Arc<WslProfiles>>>>);

impl WslCatalogCache {
    pub(crate) fn current(&self) -> Option<Arc<WslProfiles>> {
        self.0.get().and_then(Clone::clone)
    }

    pub(crate) fn ready(&self) -> anyhow::Result<Arc<WslProfiles>> {
        anyhow::ensure!(
            self.0.get().is_some(),
            "WSL profile discovery is still pending."
        );
        self.current()
            .ok_or_else(|| anyhow::anyhow!("No installed WSL profiles were discovered."))
    }

    pub(crate) fn wait(&self, cancelled: &CancellationToken) -> Option<Arc<WslProfiles>> {
        while self.0.get().is_none() && !cancelled.is_cancelled() {
            thread::sleep(Duration::from_millis(15));
        }
        self.current()
    }
}

#[derive(Default)]
pub(crate) struct WslCatalogWorker {
    pub(crate) cache: WslCatalogCache,
    cancel: Option<CancellationToken>,
    thread: Option<JoinHandle<()>>,
}

impl WslCatalogWorker {
    /// Call once as the authorized owner starts, not from a profile read.
    pub(crate) fn start(&mut self, owner: &CancellationToken) {
        self.start_with(owner, |worker| {
            WslProfiles::discover(|| worker.is_cancelled())
                .ok()
                .map(Arc::new)
        });
    }

    fn start_with(
        &mut self,
        owner: &CancellationToken,
        discover: impl FnOnce(CancellationToken) -> Option<Arc<WslProfiles>> + Send + 'static,
    ) {
        if self.cancel.is_some() {
            return;
        }
        let cancel = owner.child_token();
        let worker = cancel.clone();
        self.cancel = Some(cancel);
        let cache = self.cache.clone();
        self.thread = thread::Builder::new()
            .name("shellfox-wsl-catalog".into())
            .spawn(move || {
                let _ = cache.0.set(discover(worker));
            })
            .ok();
        if self.thread.is_none() {
            let _ = self.cache.0.set(None);
        }
    }

    pub(crate) fn started_cache(&self) -> Option<WslCatalogCache> {
        self.cancel.as_ref().map(|_| self.cache.clone())
    }

    pub(crate) fn poll(&mut self) -> bool {
        if !self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            return false;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // A failed worker must not leave catalog readers waiting indefinitely.
        let _ = self.cache.0.set(None);
        true
    }

    pub(crate) fn running(&self) -> bool {
        self.thread.is_some()
    }

    pub(crate) fn cancel(&self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }
}

impl Drop for WslCatalogWorker {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    #[test]
    fn negative_discovery_is_cached_for_repeated_reads_and_start_calls() {
        let owner = CancellationToken::new();
        let mut worker = WslCatalogWorker::default();
        let calls = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let calls = calls.clone();
            worker.start_with(&owner, move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                None
            });
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker.poll() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(!worker.running());
        assert!(worker.cache.wait(&owner).is_none());
        assert!(
            worker
                .cache
                .ready()
                .err()
                .unwrap()
                .to_string()
                .contains("No installed")
        );
        assert!(worker.cache.current().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
