//! What holds a Linux terminal's process tree. A cgroup v2 scope in the
//! systemd user delegation when the account has one: it kills everything at
//! once and isolates OOM. Otherwise (root without a user manager, no systemd,
//! cgroup v1, containers) the controller's session, swept on close.
use super::cgroup::Cgroup;
use super::linux::{inspection::Membership, sweep};
use anyhow::{ensure, Context, Result};
use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Test builds only: `session` pretends the user delegation is missing.
#[cfg(feature = "test-support")]
pub const FORCE_SESSION: &str = "SHELLFOX_TEST_LINUX_CONTAINMENT";
/// A closing controller sweeps its own session; the host only backs it up.
const CONTROLLER_GRACE: Duration = Duration::from_secs(2);
const SWEEP_INTERVAL: Duration = Duration::from_millis(20);
/// Backstop between member exit events, which can't cover new members.
const MEMBER_RECHECK: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Containment {
    Cgroup,
    Session,
}
impl Containment {
    pub(super) fn argument(self) -> &'static str {
        match self {
            Self::Cgroup => "cgroup",
            Self::Session => "session",
        }
    }
    pub(super) fn parse(value: Option<&std::ffi::OsStr>) -> Result<Self> {
        match value.and_then(|value| value.to_str()) {
            Some("cgroup") => Ok(Self::Cgroup),
            Some("session") => Ok(Self::Session),
            _ => anyhow::bail!("invalid containment"),
        }
    }
}

struct Selection {
    containment: Containment,
    /// The probed systemd-run, only for cgroup containment.
    runner: Option<PathBuf>,
}

/// Chosen once per process, on first use, and logged.
pub fn containment() -> Containment {
    selection().containment
}
fn selection() -> &'static Selection {
    static SELECTION: OnceLock<Selection> = OnceLock::new();
    SELECTION.get_or_init(|| {
        #[cfg(feature = "test-support")]
        let forced = std::env::var_os(FORCE_SESSION).is_some_and(|value| value == "session");
        #[cfg(not(feature = "test-support"))]
        let forced = false;
        let (selection, reason) = choose(forced, || Cgroup::create().map(drop), probe_runner);
        eprintln!(
            "Terminal containment: {} ({reason})",
            selection.containment.argument()
        );
        selection
    })
}
fn choose(
    forced: bool,
    delegation: impl FnOnce() -> Result<()>,
    runner: impl FnOnce() -> Result<PathBuf>,
) -> (Selection, String) {
    let session = |reason: String| {
        let selection = Selection {
            containment: Containment::Session,
            runner: None,
        };
        (selection, reason)
    };
    if forced {
        return session("forced for tests".into());
    }
    if let Err(error) = delegation() {
        return session(format!("{error:#}"));
    }
    match runner() {
        Ok(runner) => (
            Selection {
                containment: Containment::Cgroup,
                runner: Some(runner),
            },
            "systemd user delegation".into(),
        ),
        Err(error) => session(format!("{error:#}")),
    }
}
/// The same scope options as a launch, tried on a disposable `true`.
fn probe_runner() -> Result<PathBuf> {
    let runner = super::private::helper_path(Path::new("/usr/bin/systemd-run"))?;
    let program = ["/usr/bin/true", "/bin/true"]
        .iter()
        .find_map(|path| super::private::helper_path(Path::new(path)).ok())
        .context("no trusted true for the scope probe")?;
    super::linux::scope::probe(&runner, &program, &super::linux::scope::fresh_nonce()?)?
        .context("systemd-run --user --scope refused")
}
/// systemd-run needs the account's bus.
pub(super) fn user_bus() -> String {
    format!("unix:path=/run/user/{}/bus", unsafe { libc::getuid() })
}

pub(super) enum Boundary {
    Cgroup(Cgroup),
    Session(Sweep),
}
#[derive(Default)]
pub(super) struct Sweep {
    session: Option<sweep::Session>,
    root: Option<File>,
    closing: Option<Instant>,
    /// A recent scan that still found members; never caches emptiness.
    populated: Option<Instant>,
    /// The controller exited and no member was left. Final: nothing can join
    /// a session whose leader is dead and empty, and once the controller is
    /// reaped its PID may name another session.
    finished: bool,
}
impl Boundary {
    pub fn create() -> Result<Self> {
        Ok(match containment() {
            Containment::Cgroup => Self::Cgroup(Cgroup::create()?),
            Containment::Session => {
                sweep::same_pid_namespace()?;
                Self::Session(Sweep::default())
            }
        })
    }
    pub fn containment(&self) -> Containment {
        match self {
            Self::Cgroup(_) => Containment::Cgroup,
            Self::Session(_) => Containment::Session,
        }
    }
    /// The waiting controller's argv. A cgroup launch goes through the scope
    /// wrapper, which execs the same child and never expands its arguments.
    pub fn command_line(&self, controller: &Path, args: &[OsString]) -> Result<Vec<OsString>> {
        let mut argv = Vec::new();
        match self {
            Self::Cgroup(_) => {
                let runner = selection().runner.clone().context("systemd-run missing")?;
                argv.extend([runner.into_os_string(), "--expand-environment=no".into()]);
                argv.extend(super::linux::scope::scope_arguments(
                    &super::linux::scope::fresh_nonce()?,
                    controller,
                    args,
                )?);
            }
            Self::Session(_) => {
                argv.push(controller.as_os_str().to_owned());
                argv.extend_from_slice(args);
            }
        }
        Ok(argv)
    }
    /// `pid` is the waiting, unreaped controller and `pidfd` pins it.
    pub fn attach(&mut self, pid: u32, pidfd: &File) -> Result<()> {
        match self {
            Self::Cgroup(group) => group.attach(pid),
            Self::Session(sweep) => {
                ensure!(
                    unsafe { libc::getsid(pid as libc::pid_t) } == pid as libc::pid_t,
                    "controller does not lead its own session"
                );
                sweep.session = Some(sweep::Session::new(pid)?);
                sweep.root = Some(pidfd.try_clone()?);
                Ok(())
            }
        }
    }
    /// Calls `wake` as the tree tears down, then once it is empty.
    pub fn watch(&self, wake: crate::Wake) -> Result<()> {
        let sweep = match self {
            Self::Cgroup(group) => return group.watch(wake),
            Self::Session(sweep) => sweep,
        };
        let session = sweep.session.context("controller not attached")?;
        let root = sweep
            .root
            .as_ref()
            .context("controller not attached")?
            .try_clone()?;
        std::thread::Builder::new()
            .name("shellfox-session-watch".into())
            .spawn(move || {
                sweep::wait_exit(&root);
                wake();
                loop {
                    match session.wait(MEMBER_RECHECK) {
                        Ok(false) => break,
                        Ok(true) => (),
                        // A failed scan is not an empty session.
                        Err(_) => std::thread::sleep(MEMBER_RECHECK),
                    }
                    wake();
                }
                wake();
            })
            .context("start session watch")?;
        Ok(())
    }
    /// Starts or continues killing the tree. Call until `empty`.
    pub fn terminate(&mut self) -> Result<()> {
        let sweep = match self {
            Self::Cgroup(group) => return group.terminate(),
            Self::Session(sweep) => sweep,
        };
        if sweep.finished {
            return Ok(());
        }
        let (session, root) = sweep
            .session
            .zip(sweep.root.as_ref())
            .context("controller not attached")?;
        let now = Instant::now();
        let closing = *sweep.closing.get_or_insert(now);
        // The controller is the subreaper, so it also catches orphans that
        // left the session; only kill it after it had time to sweep.
        let signal = if now.duration_since(closing) < CONTROLLER_GRACE {
            libc::SIGTERM
        } else {
            libc::SIGKILL
        };
        if closing == now || signal == libc::SIGKILL {
            signal_pidfd(root, signal)?;
        }
        if sweep.populated.is_none_or(|at| now >= at + SWEEP_INTERVAL) {
            sweep.populated = session.kill()?.then_some(now);
        }
        Ok(())
    }
    /// Everything but the unreaped controller is gone.
    pub fn empty(&mut self) -> Result<bool> {
        let sweep = match self {
            Self::Cgroup(group) => return group.empty(),
            Self::Session(sweep) => sweep,
        };
        let (Some(session), Some(root)) = (sweep.session, sweep.root.as_ref()) else {
            return Ok(true);
        };
        if sweep.finished {
            return Ok(true);
        }
        let now = Instant::now();
        if sweep.populated.is_some_and(|at| now < at + SWEEP_INTERVAL) {
            return Ok(false);
        }
        // Exit first, so an empty scan after it is final.
        let exited = sweep::exited(root)?;
        sweep.populated = session.populated()?.then_some(now);
        sweep.finished = exited && sweep.populated.is_none();
        Ok(sweep.populated.is_none())
    }
    pub fn remove(&mut self) -> Result<()> {
        match self {
            Self::Cgroup(group) => group.remove(),
            Self::Session(_) => {
                ensure!(self.empty()?, "terminal session is still populated");
                Ok(())
            }
        }
    }
    pub fn inspection(&self) -> Result<Membership> {
        Ok(match self {
            Self::Cgroup(group) => Membership::Cgroup(group.inspection_directory()?),
            Self::Session(sweep) => {
                Membership::Session(sweep.session.context("controller not attached")?)
            }
        })
    }
}
fn signal_pidfd(pidfd: &File, signal: i32) -> Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: a held pidfd, null siginfo and zero flags.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    };
    ensure!(
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
        "signal terminal controller: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreachable_probe<T>() -> Result<T> {
        panic!("a forced session must not probe");
    }
    #[test]
    fn tests_can_force_session_containment_without_probing() {
        let (selection, reason) = choose(true, unreachable_probe, unreachable_probe);
        assert_eq!(selection.containment, Containment::Session);
        assert!(selection.runner.is_none());
        assert_eq!(reason, "forced for tests");
    }
    #[test]
    fn a_missing_delegation_selects_the_session_and_says_why() {
        let (selection, reason) = choose(
            false,
            || anyhow::bail!("no cgroup-v2 user delegation"),
            unreachable_probe,
        );
        assert_eq!(selection.containment, Containment::Session);
        assert_eq!(reason, "no cgroup-v2 user delegation");
    }
    #[test]
    fn a_refused_scope_wrapper_selects_the_session() {
        let (selection, reason) = choose(
            false,
            || Ok(()),
            || anyhow::bail!("systemd-run --user --scope refused"),
        );
        assert_eq!(selection.containment, Containment::Session);
        assert!(selection.runner.is_none());
        assert!(reason.contains("refused"), "{reason}");
    }
    #[test]
    fn a_working_delegation_and_scope_wrapper_select_the_cgroup() {
        let runner = PathBuf::from("/usr/bin/systemd-run");
        let (selection, _) = choose(false, || Ok(()), || Ok(runner.clone()));
        assert_eq!(selection.containment, Containment::Cgroup);
        assert_eq!(selection.runner, Some(runner));
    }
    #[test]
    fn containment_arguments_round_trip_and_reject_anything_else() {
        for containment in [Containment::Cgroup, Containment::Session] {
            let argument = std::ffi::OsStr::new(containment.argument());
            assert_eq!(Containment::parse(Some(argument)).unwrap(), containment);
        }
        assert!(Containment::parse(Some("none".as_ref())).is_err());
        assert!(Containment::parse(None).is_err());
    }
    #[test]
    fn the_session_command_line_is_the_controller_itself() {
        let boundary = Boundary::Session(Sweep::default());
        let argv = boundary
            .command_line(Path::new("/opt/shellfox"), &["--flag".into()])
            .unwrap();
        assert_eq!(argv, [OsString::from("/opt/shellfox"), "--flag".into()]);
    }
}
