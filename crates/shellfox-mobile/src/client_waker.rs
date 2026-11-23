//! Tells the native owner thread that dispatch or read_event has work.
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

pub type WakeFn = unsafe extern "C" fn(*mut c_void, u64);

struct Registered {
    wake: WakeFn,
    context: *mut c_void,
}
// SAFETY: the header requires the callback and context to be callable from any
// thread until the waker is replaced or the client is closed.
unsafe impl Send for Registered {}

/// Coalesced: one callback per owner pass. `sfm_client_dispatch` re-arms it,
/// so work queued during a pass wakes the owner again.
#[derive(Default)]
pub(super) struct Waker {
    pending: AtomicBool,
    registered: Mutex<Option<Registered>>,
}
impl Waker {
    pub fn set(&self, handle: u64, wake: Option<WakeFn>, context: *mut c_void) {
        let Ok(mut registered) = self.registered.lock() else {
            return;
        };
        *registered = wake.map(|wake| Registered { wake, context });
        drop(registered);
        // Work queued before registration still needs one pass.
        self.pending.store(false, Ordering::Release);
        self.wake(handle);
    }
    pub fn rearm(&self) {
        self.pending.store(false, Ordering::Release);
    }
    pub fn wake(&self, handle: u64) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        // Holding the lock keeps the callback alive against a concurrent clear.
        if let Ok(registered) = self.registered.lock() {
            if let Some(registered) = registered.as_ref() {
                // SAFETY: header contract; the callback only posts to the owner.
                unsafe { (registered.wake)(registered.context, handle) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn count(_: *mut c_void, handle: u64) {
        assert_eq!(handle, 7);
        CALLS.fetch_add(1, Ordering::SeqCst);
    }
    #[test]
    fn wakes_once_per_pass_and_never_after_clear() {
        let waker = Waker::default();
        waker.wake(7);
        waker.set(7, Some(count), std::ptr::null_mut());
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            1,
            "registration runs one pass"
        );
        waker.wake(7);
        waker.wake(7);
        assert_eq!(CALLS.load(Ordering::SeqCst), 1, "coalesced until re-armed");
        waker.rearm();
        waker.wake(7);
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
        waker.set(7, None, std::ptr::null_mut());
        waker.rearm();
        waker.wake(7);
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    }
}
