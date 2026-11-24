//! Terminals and automation runs without a systemd user delegation, as for
//! root in a container or a distro without systemd. The delegation is
//! pretended missing, so this runs the session sweep on every Linux host.
#![cfg(all(target_os = "linux", feature = "test-support"))]
#[path = "support/raw_unix.rs"]
#[allow(dead_code)]
mod raw;
#[path = "support/unix_production.rs"]
#[allow(dead_code)]
mod support;
use shellfox_terminal::raw_process::{RawOutput, RawProcess, RawStream};
use shellfox_terminal::unix::{containment, Containment, FORCE_SESSION};
use shellfox_terminal::{PtyOutput, PtySession};
use std::time::{Duration, Instant};
use support::{collect, descendant, until, Fixture};

/// Before anything selects the containment, which happens once per process.
fn session() {
    static FORCE: std::sync::Once = std::sync::Once::new();
    FORCE.call_once(|| std::env::set_var(FORCE_SESSION, "session"));
    assert_eq!(containment(), Containment::Session);
}

#[test]
fn session_terminal_input_output_resize_and_exit() {
    session();
    let fixture = Fixture::new();
    let config = fixture.shell("read value; stty size; printf 'got:%s\\n' \"$value\"; exit 5");
    let mut session = PtySession::spawn(config).unwrap();
    session.resize(90, 30).unwrap();
    session.write(b"fallback-input\n").unwrap();
    let (output, code) = collect(&mut session);
    let text = String::from_utf8_lossy(&output);
    assert!(text.contains("30 90"), "{text}");
    assert!(text.contains("got:fallback-input"), "{text}");
    assert_eq!(code, 5);
}

#[test]
fn session_status_stays_after_the_controller_is_reaped() {
    session();
    let fixture = Fixture::new();
    let mut session = PtySession::spawn(fixture.shell("exit 3")).unwrap();
    let mut status = None;
    until(|| {
        status = session.try_wait().unwrap();
        status.is_some()
    });
    // The reaped controller's PID may name another session by now; polling
    // again must not sweep it.
    for _ in 0..5 {
        assert_eq!(session.try_wait().unwrap().unwrap().exit_code(), 3);
    }
    session.shutdown().unwrap();
}

#[test]
fn session_close_kills_a_child_that_ignores_hangup() {
    session();
    let fixture = Fixture::new();
    let pid = fixture.root.join("pid");
    let script = format!(
        "trap '' HUP; sleep 300 & echo $! > '{}'; printf 'started\\n'; wait",
        pid.display()
    );
    let mut session = PtySession::spawn(fixture.shell(&script)).unwrap();
    let mut output = Vec::new();
    until(|| {
        while let Ok(Some(PtyOutput::Data(bytes))) = session.try_read() {
            output.extend(bytes);
        }
        String::from_utf8_lossy(&output).contains("started")
    });
    let child: libc::pid_t = std::fs::read_to_string(&pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let pinned = pidfd(child);
    session.shutdown().unwrap();
    assert!(exited(&pinned), "the hangup-immune child outlived close");
}

#[test]
fn session_shutdown_kills_a_setsid_descendant_through_the_subreaper() {
    session();
    descendant(true, false, false, false);
}

#[test]
fn session_root_exit_kills_a_double_fork_orphan() {
    session();
    descendant(false, true, false, false);
}

#[test]
fn session_owner_loss_kills_the_tree_without_host_polling() {
    session();
    descendant(false, false, false, true);
}

#[test]
fn session_controller_crash_is_swept_by_the_host() {
    session();
    descendant(false, false, true, false);
}

#[test]
fn session_inspection_lists_the_shell_and_its_children() {
    session();
    let fixture = Fixture::new();
    let config = fixture.shell("sleep 30 & printf 'KIDS\\n'; wait");
    let mut session = PtySession::spawn(config).unwrap();
    let mut output = Vec::new();
    until(|| {
        while let Ok(Some(PtyOutput::Data(bytes))) = session.try_read() {
            output.extend(bytes);
        }
        String::from_utf8_lossy(&output).contains("KIDS")
    });
    let capability = session.inspection_capability().unwrap();
    let mut evidence = None;
    until(|| {
        evidence = capability
            .capture()
            .ok()
            .filter(|found| found.processes.len() >= 2);
        evidence.is_some()
    });
    let evidence = evidence.unwrap();
    assert!(evidence.root_alive);
    let root = evidence.processes.iter().find(|p| p.is_root).unwrap();
    assert!(evidence
        .processes
        .iter()
        .any(|p| p.parent_pid == Some(root.identity.pid)));
    session.shutdown().unwrap();
}

#[test]
fn session_automation_run_reports_output_and_exit() {
    session();
    let fixture = raw::Fixture::new();
    let mut process = fixture.spawn(&["echo"]);
    process
        .write(b"run".to_vec(), true)
        .unwrap()
        .wait()
        .unwrap();
    let (stdout, _, code) = raw::collect(&mut process);
    assert_eq!(stdout, b"run");
    assert_eq!(code, 7);
}

#[test]
fn session_automation_stop_kills_its_descendants() {
    session();
    let fixture = raw::Fixture::new();
    let socket = fixture.socket();
    let mut process = fixture.spawn(&["unix-branch", socket.to_str().unwrap(), "idle", "setpgid"]);
    let leaf = raw::pin_leaf(&socket);
    process.request_stop().unwrap();
    raw::until(|| leaf.exited());
    assert_eq!(raw::collect(&mut process).2, 137);
    std::fs::remove_file(socket).unwrap();
}

fn pidfd(pid: libc::pid_t) -> std::fs::File {
    use std::os::fd::FromRawFd;
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    assert!(fd >= 0, "pin {pid}");
    unsafe { std::fs::File::from_raw_fd(fd as i32) }
}
fn exited(pidfd: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    let mut poll = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::poll(&mut poll, 1, 50) } == 0 {
        if Instant::now() > deadline {
            return false;
        }
    }
    true
}

#[test]
fn session_stop_of_a_finishing_flood_is_always_the_guests_sigkill() {
    session();
    raw::stop_of_a_finishing_flood_is_the_guests_sigkill(20);
}
