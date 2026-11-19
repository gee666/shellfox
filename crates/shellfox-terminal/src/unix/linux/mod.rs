//! Linux pidfd ownership. No Python helper and no numeric PID signalling.
pub mod gate;
pub(crate) mod inspection;
mod pidfd;
mod procfs;
pub mod scope;
mod stat;
mod supervisor;

use super::tree::{Kernel, Tree};
use super::Ownership;
use anyhow::{ensure, Context, Result};
use pidfd::{LinuxKernel, Pidfd};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

const CLOSE_BUDGET: Duration = Duration::from_secs(3);

/// Keeps native identities even when startup, cleanup or rollback fails. Drop
/// transfers unconfirmed trees to a cleanup worker created before PTY allocation.
#[must_use = "retain the owner until cleanup is confirmed"]
pub struct LinuxOwner {
    kernel: LinuxKernel,
    marker: String,
    tree: Option<Tree<Pidfd>>,
    reaper: Sender<RetainedTree>,
    trusted_exit: bool,
    authenticated: bool,
    expected_image: Option<std::path::PathBuf>,
    shell: Option<super::tree::Identity>,
}
struct RetainedTree {
    tree: Tree<Pidfd>,
    trusted_exit: bool,
}
impl LinuxOwner {
    pub fn preflight(marker: &str) -> Result<Self> {
        ensure!(
            marker.len() == 36
                && marker.bytes().enumerate().all(|(i, b)| {
                    if matches!(i, 8 | 13 | 18 | 23) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                }),
            "invalid terminal marker"
        );
        let kernel = LinuxKernel::preflight()?;
        let mut cleanup_kernel = LinuxKernel::preflight()?;
        let (sender, receiver) = mpsc::channel::<RetainedTree>();
        std::thread::Builder::new()
            .name("unix-owned-cleanup".into())
            .spawn(move || {
                while let Ok(mut retained) = receiver.recv() {
                    let trusted = retained.trusted_exit
                        || !retained
                            .tree
                            .root_exited(&mut cleanup_kernel)
                            .unwrap_or(true);
                    loop {
                        if retained
                            .tree
                            .close(&mut cleanup_kernel, CLOSE_BUDGET)
                            .is_ok()
                            && trusted
                        {
                            break;
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })?;
        Ok(Self {
            kernel,
            marker: marker.to_owned(),
            tree: None,
            reaper: sender,
            trusted_exit: false,
            authenticated: false,
            expected_image: None,
            shell: None,
        })
    }

    /// Configure the final shell image before launch, including when a probed
    /// systemd-run wrapper will exec it. Never accept input on wrapper readiness.
    pub fn expect_image(&mut self, image: &std::path::Path) -> Result<()> {
        ensure!(
            self.tree.is_none(),
            "expected image must be configured before launch"
        );
        self.expected_image = Some(super::private::helper_path(image)?);
        Ok(())
    }

    /// Capture the waiting persistent supervisor before GO, never a shell that
    /// has already started running. It must still be our original direct child
    /// and isolated PTY session leader.
    pub fn capture_root(&mut self, pid: u32) -> Result<()> {
        ensure!(self.tree.is_none(), "root ownership is already captured");
        let before = procfs::record(pid, self.kernel.uid)?;
        ensure!(
            before.parent == std::process::id() && before.session == pid,
            "supervisor does not own an isolated PTY session"
        );
        let handle = self
            .kernel
            .pin(&before)?
            .context("PTY root exited before ownership capture")?;
        // Install the exact owner BEFORE marker/authentication reads can fail.
        self.tree = Some(Tree::new(before.identity.clone(), handle));
        let environment = procfs::bounded(format!("/proc/{pid}/environ"), 1024 * 1024)?;
        let expected = format!("SHELLFOX_TERMINAL_MARKER={}", self.marker);
        ensure!(
            environment
                .split(|b| *b == 0)
                .any(|entry| entry == expected.as_bytes()),
            "PTY root marker changed"
        );
        let after = self
            .kernel
            .inspect(&before.identity)?
            .context("PTY root exited during authentication")?;
        ensure!(
            before.parent == after.parent && before.session == after.session,
            "PTY root ancestry changed"
        );
        self.tree
            .as_mut()
            .context("missing tree")?
            .track(&mut self.kernel)?;
        scope::raise_owned_oom_score(pid, before.identity.ticks, scope::TERMINAL_OOM_SCORE_ADJ);
        self.authenticated = true;
        Ok(())
    }

    pub(super) fn capture_shell(&mut self, pid: u32, ticks: u64) -> Result<()> {
        let root = &self
            .tree
            .as_ref()
            .context("supervisor ownership missing")?
            .root;
        let shell = procfs::record(pid, self.kernel.uid)?;
        ensure!(
            shell.identity.ticks == ticks && shell.parent == root.pid && shell.session == root.pid,
            "supervisor shell identity changed"
        );
        self.track_descendants()?;
        self.shell = Some(shell.identity);
        Ok(())
    }
    pub fn shell_identity(&self) -> Option<(u32, String)> {
        self.shell
            .as_ref()
            .map(|shell| (shell.pid, format!("{}:{}", self.kernel.boot, shell.ticks)))
    }

    /// The host's process monitor calls this while running, including before
    /// reporting root exit. Detached descendants keep their original pidfds.
    pub fn track_descendants(&mut self) -> Result<()> {
        self.tree
            .as_mut()
            .context("root ownership missing")?
            .track(&mut self.kernel)?;
        Ok(())
    }
    pub fn root_birth(&self) -> Option<String> {
        self.tree
            .as_ref()
            .map(|tree| format!("{}:{}", self.kernel.boot, tree.root.ticks))
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn original_child_exit(&mut self, status: &portable_pty::ExitStatus) -> Result<()> {
        if self.tree.is_none() {
            return Ok(());
        }
        ensure!(
            status.signal().is_none(),
            "unexpected supervisor death cannot prove detached descendant cleanup"
        );
        self.trusted_exit = true;
        Ok(())
    }
    pub fn close(&mut self) -> Result<()> {
        self.authenticated = false;
        if let Some(tree) = self.tree.as_mut() {
            let root_gone = tree.root_exited(&mut self.kernel)?;
            tree.close(&mut self.kernel, CLOSE_BUDGET)?;
            ensure!(
                !root_gone || self.trusted_exit,
                "supervisor exit was not authenticated; ownership retained"
            );
        }
        self.tree = None;
        self.shell = None;
        Ok(())
    }
}
impl Ownership for LinuxOwner {
    fn prepare(&mut self) -> Result<()> {
        ensure!(
            self.tree.is_none(),
            "ownership cannot be prepared twice after launch"
        );
        // Actual syscall and credential checks, not a cached capability flag.
        LinuxKernel::preflight()?;
        Ok(())
    }
    fn authenticate(&mut self) -> Result<()> {
        ensure!(
            self.authenticated && self.tree.is_some(),
            "Linux root ownership was not authenticated"
        );
        let expected = self
            .expected_image
            .as_ref()
            .context("final shell image was not configured")?;
        let root = self
            .shell
            .clone()
            .context("supervisor shell was not authenticated")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let before = self
                .kernel
                .inspect(&root)?
                .context("owned root exited before shell exec")?;
            let image = std::fs::read_link(format!("/proc/{}/exe", root.pid))?;
            let after = self
                .kernel
                .inspect(&root)?
                .context("owned root exited during shell exec")?;
            ensure!(
                before.parent == after.parent && before.session == after.session,
                "root changed during shell exec"
            );
            if image == *expected {
                return self.track_descendants();
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "final shell exec was not authenticated"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn close(&mut self) -> Result<()> {
        LinuxOwner::close(self)
    }
    fn poll_root(&mut self) -> Result<bool> {
        self.track_descendants()?;
        self.tree
            .as_ref()
            .context("root ownership missing")?
            .root_exited(&mut self.kernel)
    }
}
impl Drop for LinuxOwner {
    fn drop(&mut self) {
        if let Some(tree) = self.tree.take() {
            let retained = RetainedTree {
                tree,
                trusted_exit: self.trusted_exit,
            };
            if let Err(mpsc::SendError(retained)) = self.reaper.send(retained) {
                // Preserve the descriptors if the prestarted cleanup owner died.
                std::mem::forget(retained);
            }
        }
    }
}
