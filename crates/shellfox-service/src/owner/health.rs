//! Lets the host notice an owner thread that ended, and log one that is slow.
//! A slow owner is never a reason to exit: its terminals (and the agents in
//! them) outlive any stall, whether a blocked disk, a frozen process or a
//! machine coming back from sleep.
use shellfox_protocol::CancellationToken;
use std::{
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// A pass longer than this is logged with the step it is in.
pub(crate) const SLOW_PASS: Duration = Duration::from_secs(20);
/// How often a pass that is still stuck is logged again.
const STILL_SLOW_EVERY: Duration = Duration::from_secs(60);

/// The owner pass step that runs now, named in slow-pass logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Step {
    Pass,
    Power,
    TerminalEvents,
    Closing,
    Grants,
    Catalog,
    Automation,
    Activity,
    Retention,
    Frames,
    Watches,
    Private,
    Dispatch,
    Launches,
    Task,
}
impl Step {
    const ALL: [Step; 15] = [
        Step::Pass,
        Step::Power,
        Step::TerminalEvents,
        Step::Closing,
        Step::Grants,
        Step::Catalog,
        Step::Automation,
        Step::Activity,
        Step::Retention,
        Step::Frames,
        Step::Watches,
        Step::Private,
        Step::Dispatch,
        Step::Launches,
        Step::Task,
    ];
    fn name(self) -> &'static str {
        match self {
            Step::Pass => "pass",
            Step::Power => "keep-awake",
            Step::TerminalEvents => "terminal events",
            Step::Closing => "closing terminals",
            Step::Grants => "grant retirement",
            Step::Catalog => "profile catalog",
            Step::Automation => "automation",
            Step::Activity => "activity (process poll and metadata write)",
            Step::Retention => "session retention",
            Step::Frames => "frame capture",
            Step::Watches => "watch publishing",
            Step::Private => "private requests",
            Step::Dispatch => "remote dispatch",
            Step::Launches => "launches",
            Step::Task => "request",
        }
    }
}

#[derive(Clone)]
pub(crate) struct OwnerHealth(Arc<Inner>);
struct Inner {
    ended: CancellationToken,
    start: Instant,
    /// Milliseconds since `start` when the current pass began, plus one; 0 while waiting.
    pass: AtomicU64,
    step: AtomicU8,
}
impl OwnerHealth {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Inner {
            ended: CancellationToken::new(),
            start: Instant::now(),
            pass: AtomicU64::new(0),
            step: AtomicU8::new(Step::Pass as u8),
        }))
    }
    pub(crate) fn pass_started(&self) {
        let now = self.0.start.elapsed().as_millis() as u64 + 1;
        self.0.pass.store(now, Ordering::Release);
        self.step(Step::Pass);
    }
    pub(crate) fn waiting(&self) {
        self.0.pass.store(0, Ordering::Release);
    }
    pub(crate) fn step(&self, step: Step) {
        self.0.step.store(step as u8, Ordering::Release);
    }
    fn current_step(&self) -> Step {
        let index = usize::from(self.0.step.load(Ordering::Acquire));
        Step::ALL.get(index).copied().unwrap_or(Step::Pass)
    }
    /// Marks the owner ended when dropped, including by an uncaught panic.
    pub(super) fn end_guard(&self) -> EndGuard {
        EndGuard(self.clone())
    }
    /// The running pass: when it began (in owner-clock ms) and how long it has run.
    fn current_pass(&self) -> Option<(u64, Duration)> {
        match self.0.pass.load(Ordering::Acquire) {
            0 => None,
            began => Some((
                began,
                self.0
                    .start
                    .elapsed()
                    .saturating_sub(Duration::from_millis(began - 1)),
            )),
        }
    }
    /// Resolves once the owner thread ended. Meanwhile, logs any pass slower
    /// than `slow`, again while it stays stuck, and once it finishes.
    pub(crate) async fn ended(&self, slow: Duration) {
        let check = (slow / 4).max(Duration::from_millis(10));
        // The slow pass being reported, and when it was last logged.
        let mut reported: Option<(u64, Duration)> = None;
        loop {
            tokio::select! {
                _ = self.0.ended.cancelled() => return,
                _ = tokio::time::sleep(check) => {}
            }
            let current = self.current_pass();
            match (current, reported) {
                (Some((began, ran)), Some((seen, logged))) if began == seen => {
                    if ran.saturating_sub(logged) >= STILL_SLOW_EVERY {
                        self.log_slow(ran, "is still running");
                        reported = Some((began, ran));
                    }
                }
                (_, Some((_, logged))) => {
                    eprintln!(
                        "Shellfox host: the service owner answers again after a pass of more than {} s.",
                        logged.as_secs()
                    );
                    reported = None;
                }
                (Some((began, ran)), None) if ran > slow => {
                    self.log_slow(ran, "has been running");
                    reported = Some((began, ran));
                }
                _ => {}
            }
        }
    }
    fn log_slow(&self, ran: Duration, state: &str) {
        eprintln!(
            "Shellfox host: a service owner pass {state} for {} s, in step: {}. The host keeps running and waits for it.",
            ran.as_secs(),
            self.current_step().name()
        );
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
    async fn only_an_ended_owner_ends_the_watch() {
        let health = OwnerHealth::new();
        health.pass_started();
        health.step(Step::Activity);
        assert_eq!(health.current_step(), Step::Activity);
        let slow = Duration::from_millis(40);
        let stuck = tokio::time::timeout(Duration::from_millis(300), health.ended(slow)).await;
        assert!(stuck.is_err(), "a stuck pass never ends the watch");
        health.waiting();
        let idle = tokio::time::timeout(Duration::from_millis(150), health.ended(slow)).await;
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
        tokio::time::timeout(Duration::from_secs(5), health.ended(slow))
            .await
            .expect("an ended owner ends the watch");
    }
}
