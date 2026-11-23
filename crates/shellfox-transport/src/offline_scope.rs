use crate::{
    offline_control::OfflineControl, BoundOwnershipRoot, HelperError, NoOwnerProof,
    OwnershipRootIdentity, RootLifetimeGuard, VerifiedHelper,
};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex, MutexGuard},
    thread,
    time::Duration,
};
const MAX_PENDING: usize = 32;
pub(crate) struct Ledger {
    accepting: bool,
    attempted: bool,
    owner: Option<Arc<OfflineControl>>,
    root: Option<BoundOwnershipRoot>,
    children: Vec<Arc<OfflineControl>>,
    original_ids: Vec<OwnershipRootIdentity>,
    originals_required: bool,
}
impl Ledger {
    pub fn check_owner(&self, expected: &Arc<OfflineControl>) -> Result<(), HelperError> {
        if !self.accepting
            || self.root.is_none()
            || !self
                .owner
                .as_ref()
                .is_some_and(|owner| Arc::ptr_eq(owner, expected))
        {
            return Err(HelperError::OwnershipRequired);
        }
        expected.check_cached_owner()
    }
    fn check_protection(&self, protect: bool) -> Result<(), HelperError> {
        if protect && self.originals_required {
            self.check_owner(self.owner.as_ref().ok_or(HelperError::OwnershipRequired)?)?;
        }
        Ok(())
    }
    fn check_originals(&self) -> Result<(), HelperError> {
        if self.originals_required && self.original_ids.is_empty() {
            return Err(HelperError::RootChainRequired);
        }
        Ok(())
    }
}
struct Inner {
    ledger: Mutex<Ledger>,
    send: mpsc::SyncSender<Arc<OfflineControl>>,
    original_directories: RootLifetimeGuard,
    helper: VerifiedHelper,
}
#[derive(Clone)]
pub(crate) struct OfflineScope(Arc<Inner>);
impl OfflineScope {
    pub fn new(helper: VerifiedHelper, originals_required: bool) -> Result<Self, HelperError> {
        let mut children = Vec::new();
        children
            .try_reserve_exact(MAX_PENDING)
            .map_err(|_| HelperError::Spawn)?;
        let (send, receive) = mpsc::sync_channel::<Arc<OfflineControl>>(MAX_PENDING);
        thread::Builder::new()
            .name("shellfox-offline-preparation".into())
            .spawn(move || monitor(receive))
            .map_err(|_| HelperError::Spawn)?;
        Ok(Self(Arc::new(Inner {
            ledger: Mutex::new(Ledger {
                accepting: true,
                attempted: false,
                owner: None,
                root: None,
                children,
                original_ids: Vec::with_capacity(256),
                originals_required,
            }),
            send,
            original_directories: RootLifetimeGuard::new(),
            helper,
        })))
    }
    pub fn forward(&self) -> Result<MutexGuard<'_, Ledger>, HelperError> {
        self.admission(false)
    }
    fn admission(&self, protect: bool) -> Result<MutexGuard<'_, Ledger>, HelperError> {
        let ledger = self.0.ledger.lock().map_err(|_| HelperError::Stopped)?;
        ledger.check_protection(protect)?;
        if !ledger.accepting {
            return Err(HelperError::Stopped);
        }
        Ok(ledger)
    }
    pub fn retain_original_directories(
        &self,
        files: Vec<std::fs::File>,
    ) -> Result<(), HelperError> {
        let mut ledger = self.forward()?;
        let ids = self.0.original_directories.append(files)?;
        ledger.original_ids.extend(ids);
        Ok(())
    }
    pub fn root_lifetime(&self) -> RootLifetimeGuard {
        self.0.original_directories.clone()
    }
    pub fn bind(&self, root: &BoundOwnershipRoot) -> Result<(), HelperError> {
        let mut ledger = self.forward()?;
        if ledger.root.is_some() {
            return Err(HelperError::Stopped);
        }
        if ledger.original_ids.len() < root.directory_components()
            || ledger.original_ids.last() != Some(root.identity())
        {
            return Err(HelperError::RootChainRequired);
        }
        ledger.root = Some(root.clone());
        Ok(())
    }
    pub fn begin_acquire(&self) -> Result<BoundOwnershipRoot, HelperError> {
        let mut ledger = self.forward()?;
        if ledger.attempted {
            return Err(HelperError::Stopped);
        }
        let root = ledger.root.clone().ok_or(HelperError::RootChainRequired)?;
        ledger.attempted = true;
        Ok(root)
    }
    pub fn mark_owned(&self, control: &Arc<OfflineControl>) -> Result<(), HelperError> {
        let mut ledger = self.forward()?;
        if ledger.owner.is_some()
            || !ledger.attempted
            || !ledger
                .children
                .iter()
                .any(|child| Arc::ptr_eq(child, control))
        {
            return Err(HelperError::OwnershipRequired);
        }
        control.check_cached_owner()?;
        ledger.owner = Some(control.clone());
        Ok(())
    }
    pub fn acquired(&self) -> Result<(BoundOwnershipRoot, Arc<OfflineControl>), HelperError> {
        let ledger = self.forward()?;
        let owner = ledger
            .owner
            .as_ref()
            .ok_or(HelperError::OwnershipRequired)?;
        ledger.check_owner(owner)?;
        Ok((
            ledger.root.clone().ok_or(HelperError::RootChainRequired)?,
            owner.clone(),
        ))
    }
    /// Phase transfer only, not admission: a lost/released acquired original
    /// still needs its compatibility cleanup facade. Never use this as a fence.
    pub fn recorded_acquired(
        &self,
    ) -> Result<(BoundOwnershipRoot, Arc<OfflineControl>), HelperError> {
        let ledger = self.0.ledger.lock().unwrap_or_else(|e| e.into_inner());
        Ok((
            ledger.root.clone().ok_or(HelperError::RootChainRequired)?,
            ledger.owner.clone().ok_or(HelperError::OwnershipRequired)?,
        ))
    }
    #[cfg(unix)]
    pub fn native<T>(
        &self,
        protect: bool,
        operation: impl FnOnce() -> Result<T, HelperError>,
    ) -> Result<T, HelperError> {
        let ledger = self.admission(protect)?; // Leaf protection check under THIS guard.
        ledger.check_originals()?;
        let result = operation();
        drop(ledger);
        result
    }
    pub fn spawn(&self, args: &[&Path], protect: bool) -> Result<Arc<OfflineControl>, HelperError> {
        self.0.helper.verify()?;
        // Command/control storage and argv are ready BEFORE entering OS spawn.
        let mut command = Command::new(self.0.helper.path());
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut ledger = self.admission(protect)?;
        ledger.check_originals()?;
        // Never erase an uncertain or merely released original. Monitor/cleanup
        // alone records positive wait; typed pre-entry cancellation is also final.
        ledger.children.retain(|child| !child.resolved_cached());
        if ledger.children.len() >= MAX_PENDING {
            return Err(HelperError::Busy);
        }
        let control = OfflineControl::prepare(
            self.0.helper.clone(),
            ledger.root.clone(),
            self.root_lifetime(),
        );
        ledger.children.push(control.clone()); // Capacity was reserved before any spawn.
        if self.0.send.send(control.clone()).is_err() {
            control.cancel_before_entry()?; // Proven locally: spawn has NOT been entered.
            return Err(HelperError::Spawn);
        }
        control.enter_spawn()?;
        let child = command.spawn().map_err(|_| HelperError::Spawn)?;
        control.capture(child); // First action after success; no allocation/pipe/task setup.
        let waiter = control.clone();
        // Without a waiter the exit is never recorded: waits time out and the
        // scope keeps the original's resources.
        let _ = thread::Builder::new()
            .name("shellfox-offline-wait".into())
            .spawn(move || waiter.wait_os_exit());
        Ok(control)
    }
    pub fn seal(&self) {
        self.0
            .ledger
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .accepting = false;
    }
    pub fn close(&self) {
        let children = {
            let mut ledger = self.0.ledger.lock().unwrap_or_else(|e| e.into_inner());
            ledger.accepting = false;
            ledger.children.clone()
        };
        for child in children {
            let _ = child.release(false);
        }
    }
    pub async fn cleanup_confirmed(&self) -> Result<NoOwnerProof, HelperError> {
        self.close();
        let children = self
            .0
            .ledger
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .children
            .clone();
        // Poison is retained unknown admission, not evidence of no effect.
        let poisoned = || self.0.ledger.is_poisoned();
        if poisoned() {
            return Err(HelperError::Stopped);
        }
        let end = tokio::time::Instant::now() + Duration::from_secs(2);
        for child in &children {
            child.wait_resolved(end).await?;
        }
        if poisoned() {
            return Err(HelperError::Stopped);
        }
        Ok(NoOwnerProof::sealed())
    }
}
/// Holds every original until the scope is gone, then releases them and keeps
/// each one's resources until its exit is recorded.
fn monitor(receive: mpsc::Receiver<Arc<OfflineControl>>) {
    let mut originals: Vec<Arc<OfflineControl>> = Vec::with_capacity(MAX_PENDING);
    while let Ok(control) = receive.recv() {
        originals.retain(|control| !control.resolved_cached());
        originals.push(control);
    }
    for control in &originals {
        let _ = control.release(false);
    }
    for control in &originals {
        control.wait_resolved_blocking();
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        let ledger = self.ledger.get_mut().unwrap_or_else(|e| e.into_inner());
        ledger.accepting = false;
        for child in &ledger.children {
            let _ = child.release(false);
        }
    }
}

#[cfg(test)]
mod tests;
