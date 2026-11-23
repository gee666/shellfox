//! Counts owner wakes so a test can block until the next one.
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

#[derive(Default)]
pub struct Wakes {
    count: Mutex<u64>,
    changed: Condvar,
}
impl Wakes {
    pub fn wake(self: &Arc<Self>) -> Arc<dyn Fn() + Send + Sync> {
        let wakes = Arc::clone(self);
        Arc::new(move || {
            *wakes.count.lock().unwrap() += 1;
            wakes.changed.notify_all();
        })
    }
    pub fn count(&self) -> u64 {
        *self.count.lock().unwrap()
    }
    /// Fails the test if nothing wakes the owner within the bound.
    pub fn wait_past(&self, seen: u64) {
        let count = self.count.lock().unwrap();
        let (_count, timeout) = self
            .changed
            .wait_timeout_while(count, Duration::from_secs(10), |count| *count == seen)
            .unwrap();
        assert!(!timeout.timed_out(), "the owner went quiet without a wake");
    }
}
