use shellfox_terminal::{PtyCommand, PtyConfig, PtyOutput, PtySession};
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::{
    fs::{self, File},
    os::fd::{AsRawFd, FromRawFd},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Fixture {
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        fs::create_dir_all(&base).unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!("pty-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root: root.canonicalize().unwrap(),
        }
    }
    pub fn config(&self, program: &Path, args: &[&str]) -> PtyConfig {
        let mut config = PtyConfig::isolated(&self.root);
        config.supervisor = Some(helper());
        config.command = Some(PtyCommand {
            program: program.into(),
            args: args.iter().map(Into::into).collect(),
        });
        config
    }
    pub fn shell(&self, script: &str) -> PtyConfig {
        self.config(Path::new("/bin/sh"), &["-c", script])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}
pub fn helper() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_shellfox-unix-test-supervisor"))
}
pub fn until(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !f() {
        assert!(
            Instant::now() < deadline,
            "actual PTY observation timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub fn collect(session: &mut PtySession) -> (Vec<u8>, u32) {
    let mut bytes = Vec::new();
    let mut eof = false;
    let mut status = None;
    until(|| {
        if status.is_none() {
            status = session.try_wait().unwrap();
        }
        loop {
            match session.try_read() {
                Ok(Some(PtyOutput::Data(data))) => bytes.extend(data),
                Ok(Some(PtyOutput::Eof)) => {
                    eof = true;
                    break;
                }
                Ok(Some(PtyOutput::ReadError(error))) => panic!("{error}"),
                _ => break,
            }
        }
        eof && status.is_some()
    });
    let code = status.unwrap().exit_code();
    session.shutdown().unwrap();
    (bytes, code)
}
/// Actual held file identity and measured bytes, not a synthetic validity flag.
fn pin_peer(stream: &UnixStream) -> File {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    assert_eq!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        },
        0
    );
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, cred.pid, 0) };
    assert!(fd >= 0);
    unsafe { File::from_raw_fd(i32::try_from(fd).unwrap()) }
}
fn exited(pidfd: &File) -> bool {
    let mut poll = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, 0) == 1 && poll.revents & libc::POLLIN != 0 }
}
pub fn controller_signal(signal: i32) {
    let fixture = Fixture::new();
    let socket = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .canonicalize()
        .unwrap()
        .join(format!("signal-leaf-{}.sock", std::process::id()));
    let exit = fixture.root.join("must-not-exit");
    let args = [
        "--fixture-root",
        socket.to_str().unwrap(),
        exit.to_str().unwrap(),
    ];
    let mut session = PtySession::spawn(fixture.config(&helper(), &args)).unwrap();
    let mut connection = None;
    until(|| {
        connection = UnixStream::connect(&socket).ok();
        connection.is_some()
    });
    let peer = pin_peer(connection.as_ref().unwrap());
    assert!(!exited(&peer));
    // This hook sends through the ORIGINAL controller pidfd captured before GO.
    // No PID lookup, replacement handle, host poll or shutdown can prove this exit.
    session.signal_controller_for_test(signal).unwrap();
    until(|| exited(&peer));
    assert!(!exit.exists());
    session.shutdown().unwrap();
    fs::remove_file(socket).unwrap();
}
pub fn descendant(shutdown: bool, double_fork: bool, crash_controller: bool, channel_loss: bool) {
    let fixture = Fixture::new();
    // Keep the filesystem socket short; the production startup uses an abstract
    // capability socket and does not depend on project path lengths.
    let socket = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp")
        .canonicalize()
        .unwrap()
        .join(format!("leaf-{}.sock", std::process::id()));
    let exit = fixture.root.join("exit");
    let args = [
        if crash_controller {
            "--fixture-crash-controller"
        } else if double_fork {
            "--fixture-double-root"
        } else {
            "--fixture-root"
        },
        socket.to_str().unwrap(),
        exit.to_str().unwrap(),
    ];
    let config = fixture.config(&helper(), &args);
    let mut session = PtySession::spawn(config).unwrap();
    let mut connection = None;
    until(|| {
        connection = UnixStream::connect(&socket).ok();
        connection.is_some()
    });
    let peer = pin_peer(connection.as_ref().unwrap());
    assert!(!exited(&peer));
    if channel_loss {
        session.disconnect_controller_for_test();
        // Do not poll State or request shutdown first. The persistent controller
        // must kill its owned cgroup on real channel loss without host assistance.
        until(|| exited(&peer));
        session.shutdown().unwrap();
    } else if shutdown {
        session.begin_shutdown().unwrap();
        assert!(session.write(b"must-not-deliver").is_err());
        session.shutdown().unwrap();
    } else {
        fs::write(&exit, b"exit").unwrap();
        let (_, code) = collect(&mut session);
        if crash_controller {
            assert_ne!(code, 0);
        } else {
            assert_eq!(code, 7);
        }
    }
    until(|| exited(&peer));
    fs::remove_file(socket).unwrap();
}
