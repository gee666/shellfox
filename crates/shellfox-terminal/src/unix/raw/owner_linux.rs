//! Linux boundary: the cgroup v2 scope or the controller's own session (see
//! unix/boundary.rs), and the original controller's pidfd.
use crate::{
    launch_privilege::{NativeLaunchBinding, NativeLaunchEvidence},
    raw_process::RawCommand,
    unix::{boundary::Boundary, linux::sweep, startup::Gate},
    UnixPrivilegeMode,
};
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    path::Path,
    process::{Child, Command},
};

pub(super) struct Launch {
    pub command: Command,
    pub owner: Owner,
    pub gate: Control,
}
pub(super) struct Control(Gate);
impl Control {
    pub fn release(
        &mut self,
        pid: u32,
        payload: &[u8],
        before_go: impl FnOnce() -> Result<()>,
    ) -> Result<UnixStream> {
        self.0.release_raw(pid, payload, before_go)
    }
}

pub(super) fn prepare(supervisor: &Path, _command: &RawCommand, bound: bool) -> Result<Launch> {
    let supervisor = crate::unix::private::helper_path(supervisor)?;
    let group = Boundary::create()?;
    let gate = Gate::new()?;
    // Match the ordinary shareable PTY: the pinned supervisor and the
    // target it execs must not gain privileges at exec time.
    let uid = unsafe { libc::getuid() };
    let mode = if bound && uid == unsafe { libc::geteuid() } {
        UnixPrivilegeMode::NonElevating
    } else {
        UnixPrivilegeMode::Local
    };
    let argv = group.command_line(&supervisor, &gate.raw_arguments(mode, group.containment()))?;
    // Target cwd/argv/env travel ONLY on the authenticated startup socket.
    // No frame bytes are put in target stdin or the controller environment.
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).env_clear();
    if group.containment() == crate::unix::Containment::Cgroup {
        command.env(
            "DBUS_SESSION_BUS_ADDRESS",
            crate::unix::boundary::user_bus(),
        );
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(Launch {
        command,
        owner: Owner {
            group,
            pidfd: None,
            mode,
        },
        gate: Control(gate),
    })
}

pub(super) struct Owner {
    group: Boundary,
    pidfd: Option<File>,
    mode: UnixPrivilegeMode,
}
impl Owner {
    /// Original Child is held unreaped, never a later lookup by a cached PID.
    pub fn pin(&mut self, child: &Child) -> Result<()> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
        ensure!(
            fd >= 0,
            "pin original raw controller: {}",
            std::io::Error::last_os_error()
        );
        self.pidfd = Some(unsafe { File::from_raw_fd(i32::try_from(fd)?) });
        Ok(())
    }
    pub fn attach(&mut self, child: &Child) -> Result<()> {
        let pidfd = self.pidfd.as_ref().context("missing original pidfd")?;
        self.group.attach(child.id(), pidfd)
    }
    /// Never reaps: the controller's PID also names its session until `remove`.
    pub fn exited(&mut self, _child: &mut Child) -> Result<bool> {
        sweep::exited(self.pidfd.as_ref().context("missing original pidfd")?)
    }
    /// The boundary ends every holder, so pipes always reach their actual EOF.
    pub fn reader(&self, pipe: impl Read + Send + 'static) -> Result<Box<dyn Read + Send>> {
        Ok(Box::new(pipe))
    }
    pub fn evidence(
        &self,
        binding: NativeLaunchBinding,
        child: &Child,
    ) -> Result<NativeLaunchEvidence> {
        crate::launch_privilege::capture_raw_posix(
            binding,
            child.id(),
            self.pidfd.as_ref().context("Original raw pidfd missing")?,
            self.mode,
        )
    }
    /// Calls `wake` as the tree tears down, then once it is empty.
    pub fn watch(&self, _child: &Child, wake: crate::Wake) -> Result<()> {
        self.group.watch(wake)
    }
    pub fn terminate(&mut self) -> Result<()> {
        self.group.terminate()
    }
    pub fn empty(&mut self) -> Result<bool> {
        self.group.empty()
    }
    /// Only the trusted unreleased controller exists before GO.
    pub fn stop_unreleased(&mut self, _child: &mut Child) -> Result<()> {
        if self.pidfd.is_some() {
            self.signal(libc::SIGKILL)
                .context("stop unreleased raw controller")?;
        }
        Ok(())
    }
    pub fn remove(&mut self, child: &mut Child) -> Result<()> {
        // The exit first, so a refused remove leaves nothing half done.
        ensure!(self.exited(child)?, "Raw controller has not exited");
        self.group.remove()?;
        child
            .try_wait()?
            .context("Raw controller exit not yet waitable")?;
        Ok(())
    }
    #[cfg(feature = "test-support")]
    pub fn signal_controller(&self, _child: &Child, signal: i32) -> Result<()> {
        self.signal(signal)
    }
    fn signal(&self, signal: i32) -> Result<()> {
        let fd = self.pidfd.as_ref().context("missing original pidfd")?;
        let ok = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        ensure!(
            ok == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
            "signal original raw controller"
        );
        Ok(())
    }
}
