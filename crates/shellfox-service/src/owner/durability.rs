//! Replies wait until the changes their request made are on disk.
//!
//! The Host commits in memory and journals to disk on its own thread (see
//! shellfox_host::Host::start_journal). Each request gets a cause: the owner
//! runs the request inside it, the journal's commit hook records the last
//! change made under it, and the waiting side holds the reply until the disk
//! has that change. A request that changed nothing replies at once, so reads,
//! terminal I/O and attaches never wait for a slow disk.
use shellfox_host::JournalHooks;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// The disk behind this long is logged, and again every `STILL_BEHIND`.
const BEHIND: Duration = Duration::from_secs(30);
const STILL_BEHIND: Duration = Duration::from_secs(60);
use tokio::sync::watch;

/// Names one request across the owner and its waiting caller.
pub(crate) type Cause = u64;

#[derive(Default)]
struct State {
    /// The request the owner runs now; None for its own background work.
    current: Option<Cause>,
    /// The last change each request made that isn't on disk yet.
    pending: HashMap<Cause, u64>,
    /// The last change committed in memory.
    written: u64,
    /// Since when the disk is behind, and when that was last logged.
    behind: Option<(Instant, Option<Instant>)>,
}
pub(crate) struct Durability {
    state: Mutex<State>,
    durable: watch::Sender<u64>,
    next: AtomicU64,
}
impl Durability {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            durable: watch::channel(0).0,
            next: AtomicU64::new(1),
        })
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    pub fn cause(&self) -> Cause {
        self.next.fetch_add(1, Ordering::Relaxed)
    }
    pub fn hooks(self: &Arc<Self>) -> JournalHooks {
        let committed = Arc::downgrade(self);
        let durable = Arc::downgrade(self);
        JournalHooks {
            committed: Arc::new(move |seq| {
                if let Some(this) = committed.upgrade() {
                    let mut state = this.state();
                    state.written = seq;
                    state.behind.get_or_insert((Instant::now(), None));
                    if let Some(cause) = state.current {
                        state.pending.insert(cause, seq);
                    }
                }
            }),
            durable: Arc::new(move |seq| {
                if let Some(this) = durable.upgrade() {
                    let mut state = this.state();
                    state.pending.retain(|_, change| *change > seq);
                    if seq >= state.written {
                        if let Some((since, Some(_))) = state.behind {
                            eprintln!(
                                "Shellfox host: metadata is on disk again after {} s.",
                                since.elapsed().as_secs()
                            );
                        }
                        state.behind = None;
                    }
                    drop(state);
                    this.durable.send_replace(seq);
                }
            }),
        }
    }
    /// Logs a disk that has been behind for a while. The writer can't: it may
    /// sit in a write the kernel holds (a frozen or failing disk).
    pub fn log_lag(&self) {
        let mut state = self.state();
        let Some((since, logged)) = state.behind else {
            return;
        };
        let now = Instant::now();
        let due = match logged {
            None => now - since >= BEHIND,
            Some(logged) => now - logged >= STILL_BEHIND,
        };
        if due {
            eprintln!(
                "Shellfox host: metadata changes have waited {} s for the disk. Replies to them wait too; everything else answers.",
                (now - since).as_secs()
            );
            state.behind = Some((since, Some(now)));
        }
    }
    /// The request the owner runs now, to resume its work later (a close
    /// whose commit comes after its runtimes stopped).
    pub fn current(&self) -> Option<Cause> {
        self.state().current
    }
    /// Runs the owner's next work as `cause` until the guard drops.
    pub fn enter(self: &Arc<Self>, cause: Option<Cause>) -> Entered {
        let previous = std::mem::replace(&mut self.state().current, cause);
        Entered {
            durability: self.clone(),
            previous,
        }
    }
    /// The change the request the owner runs now must wait for, if any.
    pub fn needed(&self) -> Option<u64> {
        let state = self.state();
        state
            .current
            .and_then(|cause| state.pending.get(&cause).copied())
    }
    /// Everything committed so far, if the disk doesn't have it all yet: for
    /// a reply whose own change can't be told apart from the rest.
    pub fn barrier(&self) -> Option<u64> {
        let written = self.state().written;
        (written > *self.durable.borrow()).then_some(written)
    }
    /// Resolves once the disk has change `seq`.
    pub fn durable(&self, seq: u64) -> impl std::future::Future<Output = ()> + Send + 'static {
        let mut durable = self.durable.subscribe();
        async move {
            let _ = durable.wait_for(|durable| *durable >= seq).await;
        }
    }
    /// `settled` for a reply future that outlives this borrow.
    pub fn settle(self: &Arc<Self>, cause: Cause) -> impl std::future::Future<Output = ()> + Send {
        let this = self.clone();
        async move { this.settled(cause).await }
    }
    /// Resolves once every change `cause` made is on disk.
    pub async fn settled(&self, cause: Cause) {
        let Some(change) = self.state().pending.get(&cause).copied() else {
            return;
        };
        let mut durable = self.durable.subscribe();
        // The sender lives in self, so this wait ends only when the disk has it.
        let _ = durable.wait_for(|durable| *durable >= change).await;
        self.state().pending.remove(&cause);
    }
}
pub(crate) struct Entered {
    durability: Arc<Durability>,
    previous: Option<Cause>,
}
impl Drop for Entered {
    fn drop(&mut self) {
        self.durability.state().current = self.previous;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn only_a_request_that_changed_something_waits_for_the_disk() {
        let durability = Durability::new();
        let hooks = durability.hooks();
        let (write, read) = (durability.cause(), durability.cause());
        {
            let _write = durability.enter(Some(write));
            (hooks.committed)(1);
        }
        {
            let _read = durability.enter(Some(read));
        }
        // Background work is attributed to no request.
        (hooks.committed)(2);
        durability.settled(read).await;
        let waiting = tokio::spawn({
            let durability = durability.clone();
            async move { durability.settled(write).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        assert!(durability.state().behind.is_some());
        (hooks.durable)(1);
        waiting.await.unwrap();
        assert!(durability.state().pending.is_empty());
        // Change 2 is still in memory only.
        assert!(durability.state().behind.is_some());
        (hooks.durable)(2);
        assert!(durability.state().behind.is_none());
        assert_eq!(durability.current(), None);
    }
}
