use crate::unix::Ownership;
use anyhow::{ensure, Context, Result};
use portable_pty::{Child, ExitStatus};
use std::time::Duration;

pub(super) enum PlatformOwner {
    #[cfg(target_os = "linux")]
    Linux {
        owner: super::super::linux::LinuxOwner,
        gate: super::super::linux::gate::StartupGate,
    },
    #[cfg(target_os = "macos")]
    Darwin(super::super::darwin::PreparedSupervisor),
}
pub(super) struct NativeOwner {
    pub(super) platform: PlatformOwner,
    pub(super) child: Option<Box<dyn Child + Send + Sync>>,
    pub(super) status: Option<ExitStatus>,
}
impl NativeOwner {
    fn bind(&mut self) -> Result<()> {
        let pid = self
            .child
            .as_ref()
            .context("PTY child missing")?
            .process_id()
            .context("PTY child PID missing")?;
        match &mut self.platform {
            #[cfg(target_os = "linux")]
            PlatformOwner::Linux { owner, gate } => gate.release(pid, owner),
            #[cfg(target_os = "macos")]
            PlatformOwner::Darwin(owner) => owner.bind_pid(pid),
        }
    }
    fn observe(&mut self) -> Result<bool> {
        if self.status.is_none() {
            if let Some(child) = self.child.as_mut() {
                self.status = child.try_wait()?;
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(status) = &self.status {
            let PlatformOwner::Linux { owner, .. } = &mut self.platform;
            owner.original_child_exit(status)?;
        }
        #[cfg(target_os = "macos")]
        if let Some(status) = &self.status {
            let PlatformOwner::Darwin(owner) = &mut self.platform;
            owner.original_child_exit(status)?;
        }
        Ok(self.status.is_some())
    }
}
impl Ownership for NativeOwner {
    fn prepare(&mut self) -> Result<()> {
        match &mut self.platform {
            #[cfg(target_os = "linux")]
            PlatformOwner::Linux { owner, .. } => Ownership::prepare(owner),
            #[cfg(target_os = "macos")]
            PlatformOwner::Darwin(owner) => Ownership::prepare(owner),
        }
    }
    fn authenticate(&mut self) -> Result<()> {
        self.bind()?;
        match &mut self.platform {
            #[cfg(target_os = "linux")]
            PlatformOwner::Linux { owner, .. } => owner.authenticate(),
            #[cfg(target_os = "macos")]
            PlatformOwner::Darwin(owner) => owner.authenticate(),
        }
    }
    fn close(&mut self) -> Result<()> {
        let observation = self.observe();
        match &mut self.platform {
            #[cfg(target_os = "linux")]
            PlatformOwner::Linux { owner, .. } => owner.close()?,
            #[cfg(target_os = "macos")]
            PlatformOwner::Darwin(owner) => owner.close()?,
        }
        observation?;
        if let Some(child) = self.child.as_mut() {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while self.status.is_none() {
                self.status = child.try_wait()?;
                ensure!(
                    self.status.is_some() || std::time::Instant::now() < deadline,
                    "original child exit not confirmed"
                );
                if self.status.is_none() {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        Ok(())
    }
    fn poll_root(&mut self) -> Result<bool> {
        #[cfg(target_os = "linux")]
        {
            let PlatformOwner::Linux { owner, .. } = &mut self.platform;
            owner.track_descendants()?;
        }
        self.observe()
    }
}
