//! One protected discovery snapshot per owner. Catalog readers never start guests.
use super::WslProfiles;
use shellfox_protocol::CancellationToken;
use std::{
    future::Future,
    sync::{Arc, Condvar, Mutex, OnceLock},
    task::{Context, Wake, Waker},
    thread::{self, JoinHandle},
};

#[derive(Default)]
struct Slot {
    value: OnceLock<Option<Arc<WslProfiles>>>,
    lock: Mutex<()>,
    changed: Condvar,
}

impl Slot {
    fn notify(&self) {
        let _guard = self.lock.lock().unwrap_or_else(|poison| poison.into_inner());
        self.changed.notify_all();
    }
}

/// Wakes `wait` when its cancellation token fires.
impl Wake for Slot {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
}

#[derive(Clone, Default)]
pub(crate) struct WslCatalogCache(Arc<Slot>);

impl WslCatalogCache {
    pub(crate) fn current(&self) -> Option<Arc<WslProfiles>> {
        self.0.value.get().and_then(Clone::clone)
    }

    /// First result wins, as with `OnceLock::set`.
    fn set(&self, profiles: Option<Arc<WslProfiles>>) {
        let _ = self.0.value.set(profiles);
        self.0.notify();
    }

    pub(crate) fn ready(&self) -> anyhow::Result<Arc<WslProfiles>> {
        anyhow::ensure!(
            self.0.value.get().is_some(),
            "WSL profile discovery is still pending."
        );
        self.current()
            .ok_or_else(|| anyhow::anyhow!("No installed WSL profiles were discovered."))
    }

    pub(crate) fn wait(&self, cancelled: &CancellationToken) -> Option<Arc<WslProfiles>> {
        let mut cancelled = std::pin::pin!(cancelled.clone().cancelled_owned());
        let waker = Waker::from(self.0.clone());
        let mut cx = Context::from_waker(&waker);
        let mut guard = self.0.lock.lock().unwrap_or_else(|poison| poison.into_inner());
        // Checks run under the lock that `set` and cancellation take to notify,
        // so neither wake-up can slip in between a check and the wait.
        while self.0.value.get().is_none() && cancelled.as_mut().poll(&mut cx).is_pending() {
            guard = self.0.changed.wait(guard).unwrap_or_else(|poison| poison.into_inner());
        }
        drop(guard);
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
    /// `wake` drops, waking the owner, when discovery finishes.
    pub(crate) fn start(&mut self, owner: &CancellationToken, wake: crate::owner::wait::WakeOnExit) {
        self.start_with(owner, move |worker| {
            let _wake = wake;
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
                cache.set(discover(worker));
            })
            .ok();
        if self.thread.is_none() {
            self.cache.set(None);
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
        self.cache.set(None);
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
    use std::time::{Duration, Instant};

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

    #[test]
    fn wait_wakes_on_cancellation_and_on_result() {
        let cache = WslCatalogCache::default();
        let cancel = CancellationToken::new();
        let waiter = {
            let (cache, cancel) = (cache.clone(), cancel.clone());
            thread::spawn(move || cache.wait(&cancel))
        };
        thread::sleep(Duration::from_millis(20));
        cancel.cancel();
        assert!(waiter.join().unwrap().is_none());

        let waiter = {
            let cache = cache.clone();
            thread::spawn(move || cache.wait(&CancellationToken::new()))
        };
        thread::sleep(Duration::from_millis(20));
        cache.set(None);
        assert!(waiter.join().unwrap().is_none());
        assert!(cache.ready().is_err_and(|error| error.to_string().contains("No installed")));
    }
}
