//! Owner-thread pacing: bounded synchronous waits and the pass interval.
use crate::Service;
use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

/// Longest pause between wait attempts. Runtime events end it early; the
/// bound covers waits on workers that can't signal (catalog, automation).
const WAIT_STEP: Duration = Duration::from_millis(2);
/// Owner pass interval while raw automation processes run: their output and
/// exit are polled from shellfox-terminal, which has no wake hook yet.
const AUTOMATION_POLL: Duration = Duration::from_millis(20);

/// Wakes the owner when dropped, so a worker's result is collected at once.
pub(crate) struct WakeOnExit(Option<shellfox_runtime::EventWake>);
impl WakeOnExit {
    pub(crate) fn new(wake: Option<shellfox_runtime::EventWake>) -> Self {
        Self(wake)
    }
}
impl Drop for WakeOnExit {
    fn drop(&mut self) {
        if let Some(wake) = &self.0 {
            wake();
        }
    }
}

/// Counts runtime event rounds so a synchronous wait can sleep until the next one.
#[derive(Default)]
pub(crate) struct RuntimeSignal {
    rounds: Mutex<u64>,
    changed: Condvar,
}
impl RuntimeSignal {
    pub(crate) fn notify(&self) {
        let mut rounds = self.rounds.lock().unwrap_or_else(|e| e.into_inner());
        *rounds = rounds.wrapping_add(1);
        self.changed.notify_all();
    }
    fn rounds(&self) -> u64 {
        *self.rounds.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Returns once a round newer than `seen` happens or `timeout` passes.
    fn wait_past(&self, seen: u64, timeout: Duration) {
        let rounds = self.rounds.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .changed
            .wait_timeout_while(rounds, timeout, |rounds| *rounds == seen);
    }
}

impl Service {
    /// How long the owner may block before its next pass. Runtime events,
    /// remote requests, worker completions and owner tasks wake it earlier.
    /// The rest are deadlines: the activity process poll (which also covers
    /// grant cancellation, inspections and desktop-launch retries), busy-quiet
    /// transitions, debounced activity persistence and a close's deadline.
    pub(crate) fn owner_pass_timeout(&self) -> Duration {
        if self.automation.busy() {
            return AUTOMATION_POLL;
        }
        let activity = &self.activity;
        let due = [activity.owner.next_tick_ms(), activity.persistence.next_flush_ms()]
            .into_iter()
            .flatten()
            .fold(activity.next_poll_ms, u64::min);
        let timeout = activity
            .owner
            .clock()
            .instant_at(due)
            .map_or(Duration::ZERO, |at| at.saturating_duration_since(Instant::now()));
        // Runtime events end the wait for a close; its deadline is the fallback.
        self.close_deadline().map_or(timeout, |deadline| {
            timeout.min(deadline.saturating_duration_since(Instant::now()))
        })
    }
    /// Sleeps until the next runtime event round, at most `timeout` (and 100 ms).
    pub(crate) fn pause_for_runtime(&self, timeout: Duration) {
        let seen = self.runtime_signal.rounds();
        self.runtime_signal.wait_past(seen, timeout.min(Duration::from_millis(100)));
    }
    /// Notifies the owner from a worker thread when it exits, even by panic.
    pub(crate) fn wake_on_exit(&self) -> WakeOnExit {
        WakeOnExit(self.event_wake.clone())
    }
    /// Every runtime installed after this, and every owner worker, calls
    /// `wake` when it has results for the owner.
    pub(crate) fn set_event_wake(&mut self, wake: shellfox_runtime::EventWake) {
        let signal = Arc::clone(&self.runtime_signal);
        let wake: shellfox_runtime::EventWake = Arc::new(move || {
            signal.notify();
            wake();
        });
        self.power.set_wake(Arc::clone(&wake));
        self.event_wake = Some(wake);
    }
    pub(crate) fn install_terminal(
        &mut self,
        tab: shellfox_core::TabId,
        terminal: crate::terminals::OwnedTerminal,
    ) {
        if let Some(wake) = &self.event_wake {
            terminal.runtime.set_event_wake(wake.clone());
        }
        self.terminals.insert(tab, terminal);
    }
    /// Runs `attempt` until it yields a value, pausing until the next runtime
    /// event round (at most [`WAIT_STEP`]) between attempts. Once `deadline`
    /// has passed after an unsuccessful attempt, returns `expired()` instead.
    pub(crate) fn wait_until<T, E>(
        &mut self,
        deadline: Instant,
        mut attempt: impl FnMut(&mut Self) -> Result<Option<T>, E>,
        expired: impl FnOnce() -> E,
    ) -> Result<T, E> {
        let signal = Arc::clone(&self.runtime_signal);
        loop {
            let seen = signal.rounds();
            if let Some(value) = attempt(self)? {
                return Ok(value);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(expired());
            }
            signal.wait_past(seen, WAIT_STEP.min(deadline - now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_owner_sleeps_until_the_next_activity_poll() {
        let data = crate::tests::scratch();
        let mut service =
            Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
        assert_eq!(service.owner_pass_timeout(), Duration::ZERO, "first poll is due");
        service.pump();
        let timeout = service.owner_pass_timeout();
        assert!(timeout > Duration::from_millis(400) && timeout <= Duration::from_millis(500));
        service.shutdown().unwrap();
    }

    #[test]
    fn a_runtime_round_ends_the_pause_early() {
        let signal = Arc::new(RuntimeSignal::default());
        let seen = signal.rounds();
        let notifier = Arc::clone(&signal);
        let thread = std::thread::spawn(move || notifier.notify());
        let started = Instant::now();
        signal.wait_past(seen, Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_ne!(signal.rounds(), seen);
        thread.join().unwrap();
        // A round already counted doesn't block the next wait either.
        signal.wait_past(seen, Duration::from_secs(5));
    }
}
