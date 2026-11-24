//! Lets the host notice an owner thread that ended or stopped answering. Without
//! it the host keeps accepting connections and fails every request, and its
//! clients never see the host as lost.
use shellfox_protocol::CancellationToken;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// One owner pass longer than this means the owner is stuck, not busy.
pub(crate) const WEDGED_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerFailure {
    /// The owner left its loop while the host was still serving.
    Ended,
    /// One pass has run longer than `WEDGED_AFTER`.
    Wedged,
}

#[derive(Clone)]
pub(crate) struct OwnerHealth(Arc<Inner>);
struct Inner {
    ended: CancellationToken,
    start: Instant,
    /// Milliseconds since `start` when the current pass began, plus one; 0 while waiting.
    pass: AtomicU64,
}
impl OwnerHealth {
    pub(super) fn new() -> Self {
        Self(Arc::new(Inner {
            ended: CancellationToken::new(),
            start: Instant::now(),
            pass: AtomicU64::new(0),
        }))
    }
    pub(super) fn pass_started(&self) {
        let now = self.0.start.elapsed().as_millis() as u64 + 1;
        self.0.pass.store(now, Ordering::Release);
    }
    pub(super) fn waiting(&self) {
        self.0.pass.store(0, Ordering::Release);
    }
    /// Marks the owner ended when dropped, including by an uncaught panic.
    pub(super) fn end_guard(&self) -> EndGuard {
        EndGuard(self.clone())
    }
    fn stuck_for(&self) -> Duration {
        match self.0.pass.load(Ordering::Acquire) {
            0 => Duration::ZERO,
            began => self
                .0
                .start
                .elapsed()
                .saturating_sub(Duration::from_millis(began - 1)),
        }
    }
    /// Resolves once the owner ended or one pass ran longer than `limit`.
    pub(crate) async fn failed(&self, limit: Duration) -> OwnerFailure {
        let check = (limit / 4).max(Duration::from_millis(10));
        loop {
            tokio::select! {
                _ = self.0.ended.cancelled() => return OwnerFailure::Ended,
                _ = tokio::time::sleep(check) => {
                    if self.stuck_for() > limit {
                        return OwnerFailure::Wedged;
                    }
                }
            }
        }
    }
}
pub(super) struct EndGuard(OwnerHealth);
impl Drop for EndGuard {
    fn drop(&mut self) {
        self.0.waiting();
        self.0 .0.ended.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_ended_or_stuck_owner_is_reported() {
        let health = OwnerHealth::new();
        health.pass_started();
        let limit = Duration::from_millis(40);
        assert_eq!(health.failed(limit).await, OwnerFailure::Wedged);
        health.waiting();
        let idle = tokio::time::timeout(Duration::from_millis(150), health.failed(limit)).await;
        assert!(
            idle.is_err(),
            "a waiting owner is healthy however long it waits"
        );
        let panicked = health.clone();
        let _ = std::thread::spawn(move || {
            let _guard = panicked.end_guard();
            panic!("owner panicked");
        })
        .join();
        assert_eq!(health.failed(limit).await, OwnerFailure::Ended);
    }
}
