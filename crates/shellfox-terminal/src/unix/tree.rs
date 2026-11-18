//! Bounded freeze/terminate transaction. Handles must retain exact identities.
use anyhow::{bail, ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const TREE_BOUND: usize = 256;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Identity {
    pub pid: u32,
    pub ticks: u64,
}
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub identity: Identity,
    pub parent: u32,
    pub session: u32,
    pub state: u8,
}
impl Record {
    pub fn dead(&self) -> bool {
        matches!(self.state, b'Z' | b'X')
    }
    pub fn stopped(&self) -> bool {
        matches!(self.state, b'T' | b't')
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Signal {
    Stop,
    Continue,
    Kill,
}

pub(crate) trait Kernel {
    type Handle;
    fn snapshot(&mut self) -> Result<Vec<Record>>;
    fn inspect(&mut self, identity: &Identity) -> Result<Option<Record>>;
    /// Must verify identity both before and after obtaining an owned handle.
    fn pin(&mut self, record: &Record) -> Result<Option<Self::Handle>>;
    fn exited(&mut self, handle: &Self::Handle) -> Result<bool>;
    fn signal(&mut self, handle: &Self::Handle, signal: Signal) -> Result<()>;
    fn pause(&mut self) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) struct Tree<H> {
    pub root: Identity,
    protected_self: bool,
    held: BTreeMap<Identity, H>,
    // Only resume processes this transaction actually stopped. Keep failures
    // across retries, including a partial termination commit.
    stopped: BTreeSet<Identity>,
}
impl<H> Tree<H> {
    pub fn new(root: Identity, handle: H) -> Self {
        Self {
            root: root.clone(),
            protected_self: false,
            held: BTreeMap::from([(root, handle)]),
            stopped: BTreeSet::new(),
        }
    }

    /// Only a persistent single-threaded supervisor may clean its own tree
    /// without stopping/killing itself. It must not fork during this operation.
    #[cfg(target_os = "linux")]
    pub fn supervisor_self(root: Identity, handle: H) -> Result<Self> {
        ensure!(
            root.pid == std::process::id(),
            "protected root must be this supervisor"
        );
        let mut tree = Self::new(root, handle);
        tree.protected_self = true;
        Ok(tree)
    }

    pub fn root_exited<K: Kernel<Handle = H>>(&self, kernel: &mut K) -> Result<bool> {
        kernel.exited(self.held.get(&self.root).context("root handle missing")?)
    }

    /// Capture while the original leader lives. Cached descendants never become
    /// SID anchors. Their exact handles remain useful after the root exits.
    pub fn track<K: Kernel<Handle = H>>(&mut self, kernel: &mut K) -> Result<bool> {
        let mut exited = Vec::new();
        for (identity, handle) in &self.held {
            if *identity != self.root && kernel.exited(handle)? {
                exited.push(identity.clone());
            }
        }
        for identity in exited {
            self.held.remove(&identity);
            self.stopped.remove(&identity);
        }
        let rows = kernel.snapshot()?;
        let root_live =
            !kernel.exited(self.held.get(&self.root).context("root handle missing")?)?;
        let root = if root_live {
            kernel.inspect(&self.root)?
        } else {
            None
        };
        if let Some(root) = &root {
            ensure!(root.session == self.root.pid, "owned root session changed");
        }
        let anchored = root
            .as_ref()
            .is_some_and(|r| r.stopped() || self.protected_self);
        let mut proven = BTreeSet::new();
        for (identity, handle) in &self.held {
            if !kernel.exited(handle)? {
                if let Some(row) = rows.iter().find(|r| r.identity == *identity) {
                    if !row.dead() {
                        proven.insert(identity.clone());
                    }
                }
            }
        }
        if anchored {
            proven.extend(
                rows.iter()
                    .filter(|r| r.session == self.root.pid && !r.dead())
                    .map(|r| r.identity.clone()),
            );
        } else if !root_live
            && rows
                .iter()
                .any(|r| r.session == self.root.pid && !r.dead() && !proven.contains(&r.identity))
        {
            bail!("owned session anchor was lost; refusing unproven members");
        }
        // Parent edges must be to a currently inspected exact identity, and
        // cannot precede that parent's birth. Never traverse numeric PID caches.
        let by_pid: BTreeMap<_, _> = rows.iter().map(|r| (r.identity.pid, r)).collect();
        for _ in 0..TREE_BOUND {
            let before = proven.len();
            for row in &rows {
                if row.dead() {
                    continue;
                }
                if by_pid.get(&row.parent).is_some_and(|parent| {
                    proven.contains(&parent.identity) && parent.identity.ticks <= row.identity.ticks
                }) {
                    proven.insert(row.identity.clone());
                }
            }
            ensure!(proven.len() <= TREE_BOUND, "owned tree bound exceeded");
            if before == proven.len() {
                break;
            }
        }
        let mut added = false;
        for row in rows.iter().filter(|r| proven.contains(&r.identity)) {
            if !self.held.contains_key(&row.identity) {
                ensure!(self.held.len() < TREE_BOUND, "held identity bound exceeded");
                if let Some(handle) = kernel.pin(row)? {
                    self.held.insert(row.identity.clone(), handle);
                    added = true;
                }
            }
        }
        Ok(added)
    }

    fn freeze<K: Kernel<Handle = H>>(&mut self, kernel: &mut K) -> Result<()> {
        for (identity, handle) in &self.held {
            if (self.protected_self && *identity == self.root) || kernel.exited(handle)? {
                continue;
            }
            let row = kernel
                .inspect(identity)?
                .context("live held identity disappeared")?;
            if !row.stopped() && !row.dead() {
                // Track before dispatch: a failure may follow successful signal
                // delivery, and rollback must still try every surviving handle.
                self.stopped.insert(identity.clone());
                kernel.signal(handle, Signal::Stop)?;
            }
        }
        Ok(())
    }

    fn rollback<K: Kernel<Handle = H>>(&mut self, kernel: &mut K) -> Result<()> {
        let mut failures = Vec::new();
        for identity in self.stopped.clone() {
            let Some(handle) = self.held.get(&identity) else {
                continue;
            };
            let result: Result<()> = (|| {
                if !kernel.exited(handle)? {
                    kernel.signal(handle, Signal::Continue)?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    self.stopped.remove(&identity);
                }
                Err(error) => failures.push(format!("{error:#}")),
            }
        }
        ensure!(
            failures.is_empty(),
            "owned survivors could not all be restored: {}",
            failures.join(", ")
        );
        Ok(())
    }

    pub fn close<K: Kernel<Handle = H>>(&mut self, kernel: &mut K, budget: Duration) -> Result<()> {
        self.rollback(kernel)?;
        let result = self.terminate(kernel, budget);
        let rollback = self.rollback(kernel);
        match (result, rollback) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (result, Err(error)) => Err(error.context(format!("cleanup result: {result:?}"))),
        }
    }

    fn terminate<K: Kernel<Handle = H>>(&mut self, kernel: &mut K, budget: Duration) -> Result<()> {
        let deadline = Instant::now() + budget;
        let mut stable = 0;
        loop {
            ensure!(Instant::now() < deadline, "owned freeze deadline exceeded");
            self.freeze(kernel)?;
            let added = self.track(kernel)?;
            self.freeze(kernel)?;
            let mut frozen = true;
            for (identity, handle) in &self.held {
                if (self.protected_self && *identity == self.root) || kernel.exited(handle)? {
                    continue;
                }
                let row = kernel
                    .inspect(identity)?
                    .context("held process disappeared during freeze")?;
                frozen &= row.stopped() || row.dead();
            }
            stable = if !added && frozen { stable + 1 } else { 0 };
            if stable >= 2 {
                break;
            }
            kernel.pause();
        }
        ensure!(
            Instant::now() < deadline,
            "owned freeze deadline exceeded before commit"
        );
        // Separate deadline after commit. Any error still resumes survivors.
        let deadline = Instant::now() + budget;
        for (identity, handle) in &self.held {
            if *identity != self.root && !kernel.exited(handle)? {
                ensure!(
                    Instant::now() < deadline,
                    "owned termination dispatch deadline exceeded"
                );
                kernel.signal(handle, Signal::Kill)?;
            }
        }
        let root = self.held.get(&self.root).context("missing root handle")?;
        if !self.protected_self && !kernel.exited(root)? {
            ensure!(
                Instant::now() < deadline,
                "owned root termination deadline exceeded"
            );
            kernel.signal(root, Signal::Kill)?;
        }
        loop {
            let mut gone = true;
            for (identity, handle) in &self.held {
                if !self.protected_self || *identity != self.root {
                    gone &= kernel.exited(handle)?;
                }
            }
            if gone {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "owned descendant exit not confirmed"
            );
            kernel.pause();
        }
    }
}
