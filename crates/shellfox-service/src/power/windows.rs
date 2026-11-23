use super::{PowerError, PowerLease};
mod lid;
use std::{
    fs::OpenOptions,
    io,
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};
use windows_sys::Win32::{
    System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    },
    UI::Input::KeyboardAndMouse::{mouse_event, MOUSEEVENTF_MOVE},
};

pub struct Lease {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
    journal: Option<PathBuf>,
}

pub(super) fn prepare(directory: &Path) -> PathBuf {
    let path = directory.join(lid::JOURNAL);
    if let Err(error) = recover_electron(directory) {
        eprintln!("Electron keep-awake recovery: {error}");
    }
    if let Err(error) = lid::recover(&path) {
        eprintln!("Windows keep-awake recovery: {error}");
    }
    path
}

fn recover_electron(directory: &Path) -> Result<(), PowerError> {
    let Ok(default) = crate::app_paths::default_data_root() else {
        return Ok(());
    };
    if !matches!((default.canonicalize(), directory.canonicalize()), (Ok(default), Ok(current)) if default == current)
    {
        return Ok(());
    }
    let Some(legacy) = default.ancestors().nth(2) else {
        return Ok(());
    };
    let journal = legacy.join(lid::JOURNAL);
    if !journal
        .try_exists()
        .map_err(|error| PowerError::operation("Inspect Electron lid journal", error))?
    {
        return Ok(());
    }
    // The shipping Electron's Go helper holds this lock while its journal is live.
    let lock = legacy.join(".host-lock/host.lock");
    lid::verify_existing_file(&lock)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock)
        .map_err(|error| PowerError::operation("Open Electron ownership lock", error))?;
    lock.try_lock()
        .map_err(|error| PowerError::operation("Electron ownership is unconfirmed", error))?;
    lid::recover(&journal)
}

fn nudge_mouse() {
    unsafe {
        mouse_event(MOUSEEVENTF_MOVE, 1, 0, 0, 0);
        mouse_event(MOUSEEVENTF_MOVE, -1, 0, 0, 0);
    }
}

pub fn acquire(journal: Option<&Path>) -> Result<Lease, PowerError> {
    let (stop, wait_stop) = mpsc::channel();
    let (ready, wait_ready) = mpsc::sync_channel(1);
    // Windows ties execution state to the calling thread, unlike the service's
    // transient workers. Retain the same thread until the lease is released.
    let uses_idle_input = journal.is_some();
    let thread = thread::Builder::new()
        .name("shellfox-windows-execution-state".into())
        .spawn(move || {
            let flags = ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED;
            if unsafe { SetThreadExecutionState(flags) } == 0 {
                let _ = ready.send(Err(PowerError::operation(
                    "SetThreadExecutionState",
                    io::Error::last_os_error(),
                )));
                return;
            }
            if ready.send(Ok(())).is_ok() {
                if uses_idle_input {
                    while matches!(
                        wait_stop.recv_timeout(Duration::from_secs(30)),
                        Err(RecvTimeoutError::Timeout)
                    ) {
                        nudge_mouse();
                    }
                } else {
                    let _ = wait_stop.recv();
                }
            }
            // Thread termination also releases the request if reset is refused.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        })
        .map_err(|error| PowerError::operation("Start execution-state worker", error))?;
    match wait_ready.recv() {
        Ok(Ok(())) => {
            let mut lease = Lease {
                stop,
                thread: Some(thread),
                journal: None,
            };
            if let Some(path) = journal {
                lid::activate(path)?;
                lease.journal = Some(path.to_owned());
                nudge_mouse();
            }
            Ok(lease)
        }
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(error) => {
            let _ = thread.join();
            Err(PowerError::operation("Read execution-state receipt", error))
        }
    }
}

impl PowerLease for Lease {
    fn release(&mut self) -> Result<(), PowerError> {
        if let Some(thread) = self.thread.take() {
            let _ = self.stop.send(());
            thread
                .join()
                .map_err(|_| PowerError("Execution-state worker panicked.".into()))?;
        }
        if let Some(path) = &self.journal {
            lid::recover(path)?;
            self.journal = None;
        }
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
