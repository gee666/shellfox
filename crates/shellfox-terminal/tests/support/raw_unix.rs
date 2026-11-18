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
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = root.join(format!("raw-unix-{}-{stamp}", std::process::id()));
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
        self.directory
            .parent()
            .unwrap()
            .join(format!("raw-leaf-{}.sock", std::process::id()))
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
pub fn pin_leaf(path: &std::path::Path) -> File {
    let mut stream = None;
    until(|| {
        stream = UnixStream::connect(path).ok();
        stream.is_some()
    });
    let stream = stream.unwrap();
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
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, peer.pid, 0) };
    assert!(fd >= 0);
    unsafe { File::from_raw_fd(i32::try_from(fd).unwrap()) }
}
pub fn exited(fd: &File) -> bool {
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, 0) == 1 && poll.revents & libc::POLLIN != 0 }
}
