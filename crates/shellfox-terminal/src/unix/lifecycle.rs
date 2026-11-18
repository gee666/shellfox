use anyhow::{ensure, Result};

/// An owner prepares its native cleanup/control resources before PTY allocation.
/// Preparation must not launch the interactive command. Authentication follows
/// child creation, and close must confirm *all* owned processes have exited.
pub trait Ownership {
    fn prepare(&mut self) -> Result<()>;
    fn authenticate(&mut self) -> Result<()>;
    fn close(&mut self) -> Result<()>;
    /// Refresh exact descendant ownership and report the original root exit.
    fn poll_root(&mut self) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Prepared,
    Allocated,
    Ready,
    CleanupPending,
    Closed,
}

/// The caller must retain this value on cleanup failure. There is deliberately
/// no Drop that signals a cached PID or silently declares cleanup successful.
#[must_use = "retain the lifecycle until close confirms owned cleanup"]
pub struct Lifecycle<O: Ownership> {
    owner: O,
    phase: Phase,
}

impl<O: Ownership> Lifecycle<O> {
    pub fn prepare(mut owner: O) -> Result<Self> {
        owner.prepare()?;
        Ok(Self {
            owner,
            phase: Phase::Prepared,
        })
    }

    /// This is the only PTY allocation entry point. A failed allocation can
    /// still leave native resources, so it disables input and retains ownership.
    pub fn allocate_pty<P>(&mut self, allocate: impl FnOnce(&mut O) -> Result<P>) -> Result<P> {
        ensure!(
            self.phase == Phase::Prepared,
            "ownership is not prepared for PTY allocation"
        );
        self.phase = Phase::CleanupPending;
        let pty = allocate(&mut self.owner)?;
        self.phase = Phase::Allocated;
        Ok(pty)
    }

    pub fn authenticate(&mut self) -> Result<()> {
        ensure!(self.phase == Phase::Allocated, "PTY has not been allocated");
        self.phase = Phase::CleanupPending;
        self.owner.authenticate()?;
        self.phase = Phase::Ready;
        Ok(())
    }

    /// Call from the host's normal terminal poll loop. An observation failure
    /// disables input and retains ownership instead of returning a false exit.
    pub fn poll(&mut self) -> Result<bool> {
        ensure!(self.phase == Phase::Ready, "PTY is not ready for polling");
        let exited = match self.owner.poll_root() {
            Ok(exited) => exited,
            Err(error) => {
                self.phase = Phase::CleanupPending;
                return Err(error);
            }
        };
        if exited {
            self.root_exited()?;
        }
        Ok(exited)
    }

    /// A root exit never means its descendants are gone.
    pub fn root_exited(&mut self) -> Result<()> {
        self.close()
    }

    pub fn close(&mut self) -> Result<()> {
        if self.phase == Phase::Closed {
            return Ok(());
        }
        self.phase = Phase::CleanupPending;
        self.owner.close()?;
        self.phase = Phase::Closed;
        Ok(())
    }

    /// A failed or partially delivered I/O operation must not leave input ready.
    pub fn input_failed(&mut self) {
        if self.phase != Phase::Closed {
            self.phase = Phase::CleanupPending;
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn input_allowed(&self) -> bool {
        self.phase == Phase::Ready
    }
    pub fn cleanup_pending(&self) -> bool {
        self.phase == Phase::CleanupPending
    }
}
