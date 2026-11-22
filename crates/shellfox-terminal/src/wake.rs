//! Notifications that let a PTY owner block instead of polling.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// Called from reader and exit-watch threads. It must not block.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Late-bound wake shared with a worker thread started before the owner registers.
#[derive(Clone, Default)]
pub(crate) struct WakeSlot(Arc<Mutex<Option<Wake>>>);

impl WakeSlot {
    pub fn set(&self, wake: Wake) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = Some(wake);
    }

    pub fn wake(&self) {
        // Clone out so the callback never runs under the slot lock.
        let wake = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(wake) = wake {
            wake();
        }
    }
}

/// Set once the root process has exited. Exit is not completion: the session
/// may still be tearing down descendants, so owners poll `try_wait` until it
/// reports the status.
#[derive(Clone, Debug, Default)]
pub struct ExitSignal(Arc<AtomicBool>);

impl ExitSignal {
    pub fn exited(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    #[cfg(any(unix, windows))]
    fn notify(&self, wake: &Wake) {
        self.0.store(true, Ordering::Release);
        wake();
    }
}

/// Calls `wake` once the child process exits, without reaping it, so the
/// session's own `try_wait` still observes the exit status.
#[cfg(unix)]
pub(crate) fn watch_exit(pid: u32, wake: Wake) -> std::io::Result<ExitSignal> {
    let pid = libc::id_t::from(pid);
    let signal = ExitSignal::default();
    let exit = signal.clone();
    std::thread::Builder::new()
        .name("shellfox-pty-exit".into())
        .spawn(move || {
            loop {
                // SAFETY: zeroed siginfo_t is a valid out-parameter for waitid.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                // SAFETY: waitid only writes into `info`. WNOWAIT leaves the
                // child waitable; ECHILD means another thread already reaped it.
                let result = unsafe {
                    libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT)
                };
                if result == 0
                    || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                {
                    break;
                }
            }
            exit.notify(&wake);
        })
        .map(|_| signal)
}

#[cfg(windows)]
pub(crate) fn watch_exit(
    process: std::os::windows::io::RawHandle,
    wake: Wake,
) -> std::io::Result<ExitSignal> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::DuplicateHandle,
        System::Threading::{
            GetCurrentProcess, WaitForSingleObject, INFINITE, SYNCHRONIZATION_SYNCHRONIZE,
        },
    };
    let mut duplicate = std::ptr::null_mut();
    // SAFETY: `process` is the live child handle owned by the session; the
    // duplicate is a new non-inheritable handle that this function owns.
    let duplicated = unsafe {
        let current = GetCurrentProcess();
        DuplicateHandle(current, process.cast(), current, &mut duplicate, SYNCHRONIZATION_SYNCHRONIZE, 0, 0)
    };
    if duplicated == 0 || duplicate.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: DuplicateHandle succeeded, so `duplicate` is open and unowned.
    let handle = unsafe { OwnedHandle::from_raw_handle(duplicate.cast()) };
    let signal = ExitSignal::default();
    let exit = signal.clone();
    std::thread::Builder::new()
        .name("shellfox-pty-exit".into())
        .spawn(move || {
            // SAFETY: `handle` stays open for the whole wait.
            unsafe { WaitForSingleObject(handle.as_raw_handle().cast(), INFINITE) };
            drop(handle);
            exit.notify(&wake);
        })
        .map(|_| signal)
}
