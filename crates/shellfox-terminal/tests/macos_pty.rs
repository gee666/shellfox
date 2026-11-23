#![cfg(target_os = "macos")]
use shellfox_terminal::{PtyCommand, PtyConfig, PtyOutput, PtySession};
use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::{
    fs::PermissionsExt,
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const READY_SOCKET: &str = "TEST_MACOS_DOUBLE_FORK_SOCKET";
const NEW_GROUP: &str = "TEST_MACOS_NEW_GROUP";

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        fs::create_dir_all(&base).unwrap();
        let dir = base.join(format!(
            "macpty-{:x}-{stamp:x}-{sequence:x}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        Self(dir.canonicalize().unwrap())
    }

    fn config(&self, executable: &Path, args: &[&str]) -> PtyConfig {
        let mut config = PtyConfig::isolated(&self.0);
        config.supervisor = Some(std::env::current_exe().unwrap());
        config.command = Some(PtyCommand {
            program: executable.into(),
            args: args.iter().map(Into::into).collect(),
        });
        config
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "macOS PTY observation timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn drain(session: &PtySession, data: &mut Vec<u8>) -> bool {
    loop {
        match session.try_read() {
            Ok(Some(PtyOutput::Data(bytes))) => data.extend(bytes),
            Ok(Some(PtyOutput::Eof)) => return true,
            Ok(Some(PtyOutput::ReadError(error))) => panic!("{error}"),
            _ => return false,
        }
    }
}

#[test]
fn interactive_input_resize_output_and_original_exit() {
    use shellfox_terminal::launch_privilege::{
        new_launch_binding, LaunchEvidenceBasis, LaunchPrivilege,
    };
    let scratch = Scratch::new();
    let mut config = scratch.config(Path::new("/bin/sh"), &["-c",
        "stty -echo; printf 'PTY-READY\\n'; IFS= read -r line; printf 'actual:%s\\n' \"$line\"; stty size; exit 7"]);
    config.unix_privilege = shellfox_terminal::UnixPrivilegeMode::NonElevating;
    let (binding, verifier) = new_launch_binding();
    let mut session = PtySession::spawn_bound(config, binding).unwrap();
    let launch = session.take_launch_evidence().unwrap();
    assert!(verifier.matches(&launch));
    assert_eq!(
        launch.facts().basis,
        LaunchEvidenceBasis::OriginalDarwinCredentials {
            uid: unsafe { libc::getuid() }
        }
    );
    assert_eq!(launch.facts().privilege, LaunchPrivilege::Normal);
    let mut output = Vec::new();
    until(|| {
        drain(&session, &mut output);
        output
            .windows(b"PTY-READY".len())
            .any(|bytes| bytes == b"PTY-READY")
    });
    session.resize(111, 37).unwrap();
    session.write(b"owned-macos-input\r").unwrap();
    let mut eof = false;
    until(|| {
        eof |= drain(&session, &mut output);
        let status = session.try_wait().unwrap();
        eof && status.is_some_and(|status| status.exit_code() == 7)
    });
    let output = String::from_utf8_lossy(&output);
    assert!(output.contains("actual:owned-macos-input"), "{output:?}");
    assert!(output.contains("37 111"), "{output:?}");
    session.shutdown().unwrap();
}

#[test]
fn explicit_close_and_leader_exit_clean_up_original_and_double_forked_groups() {
    struct Unrelated(Child);
    impl Drop for Unrelated {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut unrelated = Unrelated(Command::new("/bin/sleep").arg("30").spawn().unwrap());
    for (explicit, separate_group) in [(true, false), (true, true), (false, true)] {
        let scratch = Scratch::new();
        let path = scratch.0.join("r.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut config = scratch.config(
            &std::env::current_exe().unwrap(),
            &["--exact", "double_fork_fixture", "--nocapture"],
        );
        config
            .env
            .insert(READY_SOCKET.into(), path.into_os_string());
        if separate_group {
            config.env.insert(NEW_GROUP.into(), "1".into());
        }
        let mut session = PtySession::spawn(config).unwrap();
        let mut connection = None;
        until(|| {
            connection = listener.accept().ok().map(|(stream, _)| stream);
            connection.is_some()
        });
        let mut stream = connection.unwrap();
        // Darwin accept can inherit the listener's nonblocking mode.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut marker = [0];
        stream.read_exact(&mut marker).unwrap();
        assert_eq!(&marker, b"r");
        if explicit {
            session.begin_shutdown().unwrap();
            assert!(session.write(b"must not deliver").is_err());
        } else {
            session.write(b"exit\r").unwrap();
            until(|| {
                session
                    .try_wait()
                    .unwrap()
                    .is_some_and(|status| status.exit_code() == 7)
            });
        }
        session.shutdown().unwrap();
        assert_eq!(
            stream.read(&mut marker).unwrap(),
            0,
            "owned grandchild kept its socket open"
        );
        assert!(unrelated.0.try_wait().unwrap().is_none());
    }
}

/// Darwin keeps an exiting process in its tty close until unread output
/// drains, so closing a tab nobody reads must still finish, every time.
#[test]
fn close_with_unread_output_finishes_every_time() {
    let scratch = Scratch::new();
    for round in 0..50 {
        let config = scratch.config(
            Path::new("/bin/sh"),
            &["-c", "printf 'READY\\n'; exec /usr/bin/yes"],
        );
        let mut session = PtySession::spawn(config).unwrap();
        let mut data = Vec::new();
        until(|| {
            drain(&session, &mut data);
            data.windows(5).any(|bytes| bytes == b"READY")
        });
        // Stop reading: the output queue fills, then the tty buffer.
        std::thread::sleep(Duration::from_millis(150));
        let started = Instant::now();
        session
            .shutdown()
            .unwrap_or_else(|error| panic!("round {round}: {error:#}"));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "round {round}: close took {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn double_fork_fixture() {
    let Some(path) = std::env::var_os(READY_SOCKET) else {
        return;
    };
    let socket = UnixStream::connect(path).unwrap();
    let fd = socket.as_raw_fd();
    let separate_group = std::env::var_os(NEW_GROUP).is_some();
    // Only async-signal-safe libc calls after fork in the test harness. When
    // requested the grandchild changes groups but stays in the same SID.
    unsafe {
        let pid = libc::fork();
        if pid == 0 {
            let grandchild = libc::fork();
            if grandchild != 0 {
                libc::_exit(i32::from(grandchild < 0));
            }
            if separate_group && libc::setpgid(0, 0) != 0 {
                libc::_exit(2);
            }
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            if libc::write(fd, b"r".as_ptr().cast(), 1) != 1 {
                libc::_exit(3);
            }
            loop {
                libc::pause();
            }
        }
        assert!(pid > 1);
        assert_eq!(libc::waitpid(pid, std::ptr::null_mut(), 0), pid);
    }
    drop(socket);
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    std::process::exit(7);
}

#[test]
fn rejects_set_id_executable_before_spawn() {
    let scratch = Scratch::new();
    let path = scratch.0.join("fixture.sh");
    fs::File::create(&path)
        .unwrap()
        .write_all(b"#!/bin/sh\nexit 0\n")
        .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o4755)).unwrap();
    let error = PtySession::spawn(scratch.config(&path, &[])).unwrap_err();
    assert!(error.to_string().contains("setuid/setgid"));
    assert!(error.take_session().is_none());
}

#[test]
fn stats_capture_sees_the_shell_and_its_child_in_its_own_session_only() {
    let scratch = Scratch::new();
    let config = scratch.config(
        Path::new("/bin/sh"),
        &["-c", "sleep 30 & printf 'KIDS\n'; wait"],
    );
    let mut session = PtySession::spawn(config).unwrap();
    let mut data = Vec::new();
    until(|| {
        drain(&session, &mut data);
        String::from_utf8_lossy(&data).contains("KIDS")
    });
    // An unrelated process of this user, outside the session, is never reported.
    let mut outsider = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let capability = session.inspection_capability().unwrap();
    let mut evidence = capability.capture().unwrap();
    until(|| {
        evidence = capability.capture().unwrap();
        evidence.processes.len() >= 2
    });
    assert!(evidence.root_alive);
    let root = evidence.processes.iter().find(|p| p.is_root).unwrap();
    assert!(root.memory_bytes.is_some_and(|m| m > 0) && root.started_ms.is_some());
    assert!(evidence
        .processes
        .iter()
        .any(|p| p.parent_pid == Some(root.identity.pid)));
    assert!(evidence
        .processes
        .iter()
        .all(|p| p.identity.pid != outsider.id()));
    assert!(evidence
        .processes
        .iter()
        .all(|p| p.executable.is_none() && p.argv.is_none()));
    let _ = outsider.kill();
    let _ = outsider.wait();
    session.shutdown().unwrap();
}
