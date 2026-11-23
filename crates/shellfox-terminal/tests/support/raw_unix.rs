use super::*;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::PermissionsExt, net::UnixStream},
    },
    path::PathBuf,
};
pub struct Fixture {
    pub directory: PathBuf,
    pub helper: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        // A fresh checkout has no tmp/ yet.
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        // Tests run in parallel; a clock stamp can repeat, a counter cannot.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = root.join(format!("raw-unix-{}-{index}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            directory,
            helper: PathBuf::from(env!("CARGO_BIN_EXE_shellfox-raw-test-helper")),
        }
    }
    pub fn command(&self, args: &[&str]) -> shellfox_terminal::raw_process::RawCommand {
        shellfox_terminal::raw_process::RawCommand {
            executable: self.helper.to_str().unwrap().into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            cwd: self.directory.to_str().unwrap().into(),
            env: BTreeMap::new(),
        }
    }
    pub fn spawn(&self, args: &[&str]) -> RawProcess {
        RawProcess::spawn(&self.helper, self.command(args)).unwrap()
    }
    pub fn socket(&self) -> PathBuf {
        // Per fixture: parallel tree tests must not share one leaf socket.
        self.directory.join("leaf.sock")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.directory).unwrap();
        }
    }
}
pub fn collect(process: &mut RawProcess) -> (Vec<u8>, Vec<u8>, i32) {
    let mut bytes = [Vec::new(), Vec::new()];
    let mut eof = [false; 2];
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let code = process.poll_exit().unwrap();
        while let Some(output) = process.try_output() {
            match output {
                RawOutput::Data {
                    stream,
                    bytes: data,
                } => {
                    let index = usize::from(stream == RawStream::Stderr);
                    assert!(!eof[index], "data after actual OS EOF");
                    bytes[index].extend(data);
                }
                RawOutput::Eof(stream) => {
                    let index = usize::from(stream == RawStream::Stderr);
                    assert!(!eof[index], "duplicate EOF");
                    eof[index] = true;
                }
                RawOutput::Failed(stream) => panic!("actual pipe failed {stream:?}"),
            }
        }
        if let Some(code) = code.filter(|_| eof == [true; 2]) {
            process.close().unwrap();
            let [stdout, stderr] = bytes;
            return (stdout, stderr, code);
        }
        assert!(Instant::now() < end, "actual raw completion unconfirmed");
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub fn until(mut check: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < end, "actual raw observation timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
/// The leaf fixture, pinned so that its exit is observed, not its PID reused.
pub struct Leaf {
    pin: File,
    exited: std::cell::Cell<bool>,
}
pub fn pin_leaf(path: &std::path::Path) -> Leaf {
    let mut stream = None;
    until(|| {
        stream = UnixStream::connect(path).ok();
        stream.is_some()
    });
    let stream = stream.unwrap();
    Leaf {
        pin: pin(peer_pid(&stream)),
        exited: Default::default(),
    }
}
impl Leaf {
    pub fn exited(&self) -> bool {
        if !self.exited.get() {
            self.exited.set(observe_exit(&self.pin));
        }
        self.exited.get()
    }
}
#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> libc::pid_t {
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    assert_eq!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut peer as *mut libc::ucred).cast(),
                &mut length,
            )
        },
        0
    );
    peer.pid
}
#[cfg(target_os = "macos")]
fn peer_pid(stream: &UnixStream) -> libc::pid_t {
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    assert_eq!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut length,
            )
        },
        0
    );
    pid
}
#[cfg(target_os = "linux")]
fn pin(pid: libc::pid_t) -> File {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    assert!(fd >= 0);
    unsafe { File::from_raw_fd(i32::try_from(fd).unwrap()) }
}
#[cfg(target_os = "linux")]
fn observe_exit(pidfd: &File) -> bool {
    let mut poll = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, 0) == 1 && poll.revents & libc::POLLIN != 0 }
}
/// A kqueue with the leaf's NOTE_EXIT registered while it is still alive.
#[cfg(target_os = "macos")]
fn pin(pid: libc::pid_t) -> File {
    let queue = unsafe { File::from_raw_fd(libc::kqueue()) };
    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
    event.ident = pid as usize;
    event.filter = libc::EVFILT_PROC;
    event.flags = libc::EV_ADD | libc::EV_ONESHOT;
    event.fflags = libc::NOTE_EXIT;
    let added = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    assert_eq!(added, 0);
    queue
}
#[cfg(target_os = "macos")]
fn observe_exit(queue: &File) -> bool {
    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
    let now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::kevent(queue.as_raw_fd(), std::ptr::null(), 0, &mut event, 1, &now) == 1 }
}
