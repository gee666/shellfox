use crate::{BoundOwnershipRoot, HelperError, RootLifetimeGuard, VerifiedHelper};
use std::{
    process::{Child, ChildStderr, ChildStdout, ExitStatus},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Only the pre-entry cancellation branch can construct this receipt. Generic
/// spawn errors and unwinds have no corresponding OS no-child proof.
struct ProvenNoChild;
enum OriginalChild {
    Prepared,
    Entered,
    Captured(Child),
    NoChild(ProvenNoChild),
}
struct State {
    original: OriginalChild,
    exit: Option<ExitStatus>,
    holding: bool,
    release: bool,
    kill: bool,
    tasks: Vec<JoinHandle<()>>,
}
/// Preallocated in the original scope BEFORE OS spawn, pipe conversion or await.
pub(crate) struct OfflineControl {
    state: Mutex<State>,
    _artifact: VerifiedHelper,
    _root: Option<BoundOwnershipRoot>,
    _original_directories: RootLifetimeGuard,
    lost: CancellationToken,
    fault: std::sync::atomic::AtomicBool,
    /// Notified once `poll_exit` records the original wait; the scope monitor polls.
    exited: tokio::sync::Notify,
}
impl OfflineControl {
    pub fn prepare(
        artifact: VerifiedHelper,
        root: Option<BoundOwnershipRoot>,
        original_directories: RootLifetimeGuard,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                original: OriginalChild::Prepared,
                exit: None,
                holding: false,
                release: false,
                kill: false,
                tasks: Vec::with_capacity(2),
            }),
            _artifact: artifact,
            _root: root,
            _original_directories: original_directories,
            lost: CancellationToken::new(),
            fault: std::sync::atomic::AtomicBool::new(false),
            exited: tokio::sync::Notify::new(),
        })
    }
    pub fn enter_spawn(&self) -> Result<(), HelperError> {
        let mut s = self.state.lock().map_err(|_| HelperError::Stopped)?;
        if !matches!(s.original, OriginalChild::Prepared) {
            return Err(HelperError::Stopped);
        }
        s.original = OriginalChild::Entered;
        Ok(())
    }
    /// Immediately after spawn returns. No fallible setup may precede capture.
    pub fn capture(&self, child: Child) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.original = OriginalChild::Captured(child);
    }
    pub fn cancel_before_entry(&self) -> Result<(), HelperError> {
        let mut s = self.state.lock().map_err(|_| HelperError::Stopped)?;
        if !matches!(s.original, OriginalChild::Prepared) {
            return Err(HelperError::Stopped);
        }
        s.original = OriginalChild::NoChild(ProvenNoChild);
        Ok(())
    }
    pub fn pipes(&self) -> Result<(ChildStdout, ChildStderr), HelperError> {
        let mut s = self.state.lock().map_err(|_| HelperError::Stopped)?;
        let OriginalChild::Captured(child) = &mut s.original else {
            return Err(HelperError::Spawn);
        };
        let stdout = child.stdout.take().ok_or(HelperError::Spawn)?;
        let stderr = child.stderr.take().ok_or(HelperError::Spawn)?;
        Ok((stdout, stderr))
    }
    pub fn task(&self, task: JoinHandle<()>) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tasks
            .push(task);
    }
    pub fn fault(&self) {
        self.fault.store(true, std::sync::atomic::Ordering::Release);
        self.lost.cancel();
    }
    pub fn has_fault(&self) -> bool {
        self.fault.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn lost(&self) -> CancellationToken {
        self.lost.clone()
    }
    pub fn set_holding(&self) -> Result<(), HelperError> {
        let mut s = self.state.lock().map_err(|_| HelperError::Stopped)?;
        if s.release
            || s.exit.is_some()
            || self.has_fault()
            || !matches!(s.original, OriginalChild::Captured(_))
        {
            return Err(HelperError::Stopped);
        }
        s.holding = true;
        Ok(())
    }
    /// Leaf restriction only. Never polls the OS, takes pipes or releases a child.
    pub fn check_cached_owner(&self) -> Result<(), HelperError> {
        let s = self.state.lock().map_err(|_| HelperError::Stopped)?;
        if !s.holding
            || s.release
            || s.kill
            || s.exit.is_some()
            || !matches!(s.original, OriginalChild::Captured(_))
            || self.has_fault()
            || self.lost.is_cancelled()
        {
            return Err(HelperError::OwnershipRequired);
        }
        Ok(())
    }
    /// The only retirement evidence is typed pre-entry no-child or original wait.
    pub fn resolved_cached(&self) -> bool {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.exit.is_some() || matches!(s.original, OriginalChild::NoChild(_))
    }
    pub fn poll_exit(&self) -> Result<Option<ExitStatus>, HelperError> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(exit) = s.exit {
            return Ok(Some(exit));
        }
        let OriginalChild::Captured(child) = &mut s.original else {
            return Ok(None); // Missing captured child is not no-child evidence.
        };
        match child.try_wait() {
            Ok(Some(exit)) => {
                child.stdin.take();
                s.exit = Some(exit);
                self.lost.cancel();
                self.exited.notify_waiters();
                Ok(Some(exit))
            }
            Ok(None) => Ok(None),
            Err(_) => {
                self.lost.cancel();
                Err(HelperError::Stopped)
            }
        }
    }
    pub fn release(&self, force: bool) -> Result<(), HelperError> {
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.release = true;
            if let OriginalChild::Captured(child) = &mut s.original {
                child.stdin.take();
            }
            self.lost.cancel();
        }
        if self.poll_exit()?.is_some() || self.resolved_cached() {
            return Ok(());
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // The monitor may have positively waited between poll_exit and this lock.
        if s.exit.is_some() || matches!(s.original, OriginalChild::NoChild(_)) {
            return Ok(());
        }
        if (force || !s.holding) && !s.kill {
            let OriginalChild::Captured(child) = &mut s.original else {
                return Err(HelperError::Stopped);
            };
            child.kill().map_err(|_| HelperError::Stopped)?;
            s.kill = true;
        }
        Ok(())
    }
    /// Waits for the original wait, which the scope monitor records.
    pub async fn wait_exit(&self, timeout: Duration) -> Result<ExitStatus, HelperError> {
        let end = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.exited.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(exit) = self.poll_exit()? {
                return Ok(exit);
            }
            if tokio::time::timeout_at(end, notified).await.is_err() {
                return self.poll_exit()?.ok_or(HelperError::Deadline);
            }
        }
    }
}
