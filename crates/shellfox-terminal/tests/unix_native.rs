#![cfg(all(
    feature = "test-support",
    any(target_os = "linux", target_os = "macos")
))]
//! Explicit native acceptance, never silently substituted with a model test.
use shellfox_terminal::unix::native_session::{NativeSession, PreparedLaunch};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MARKER: &str = "12345678-1234-1234-1234-123456789abc";
struct Fixture {
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&root).unwrap();
        let unique = format!(
            "unix-native-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = root.join(unique);
        std::fs::create_dir(&path).unwrap();
        Self {
            path: path.canonicalize().unwrap(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
struct Unrelated(Child);
impl Drop for Unrelated {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn helper() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_shellfox-unix-test-supervisor"))
}
fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !check() {
        assert!(
            Instant::now() < deadline,
            "native fixture deadline exceeded"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn alive(endpoint: &Path) -> bool {
    UnixStream::connect(endpoint).is_ok()
}
fn control_directories(f: &Fixture) -> std::collections::BTreeSet<OsString> {
    std::fs::read_dir(f.path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.as_encoded_bytes().starts_with(b"sfx-"))
        .collect()
}
fn prepare(f: &Fixture, command: &Path, args: &[OsString]) -> PreparedLaunch {
    #[cfg(target_os = "linux")]
    {
        PreparedLaunch::linux(f.path.parent().unwrap(), &helper(), command, args, MARKER).unwrap()
    }
    #[cfg(target_os = "macos")]
    {
        let supervisor = PathBuf::from(
            std::env::var_os("SHELLFOX_DARWIN_SUPERVISOR")
                .expect("set the verified built active C supervisor path"),
        );
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        PreparedLaunch::darwin(
            f.path.parent().unwrap(),
            &supervisor,
            command,
            args,
            MARKER,
            arch,
        )
        .unwrap()
    }
}
fn spawn(f: &Fixture, args: &[OsString]) -> NativeSession {
    let launch = prepare(f, &helper(), args);
    NativeSession::spawn(
        launch,
        &f.path,
        &BTreeMap::new(),
        portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
    )
    .unwrap()
}
#[test]
#[ignore = "requires native Unix kernel and built verified platform helper; run with --ignored"]
fn native_close_and_root_exit_contain_descendants_but_not_unrelated_processes() {
    for natural_exit in [false, true] {
        let f = Fixture::new();
        let owned = f.path.join("owned");
        let unrelated = f.path.join("other");
        let exit = f.path.join("exit");
        let mut unrelated_child = Unrelated(
            Command::new(helper())
                .arg("--fixture-leaf")
                .arg(&unrelated)
                .spawn()
                .unwrap(),
        );
        until(|| alive(&unrelated));
        let args = [
            OsString::from("--fixture-root"),
            owned.clone().into_os_string(),
            exit.clone().into_os_string(),
        ];
        let controls_before = control_directories(&f);
        let mut session = spawn(&f, &args);
        until(|| alive(&owned));
        if natural_exit {
            // Do not give the host monitor a chance to discover this child.
            // The persistent supervisor must retain it across shell/root exit.
            std::fs::write(exit, b"exit").unwrap();
            until(|| session.poll().unwrap());
        } else {
            assert!(!session.poll().unwrap());
            session.close().unwrap();
        }
        until(|| !alive(&owned));
        assert!(alive(&unrelated));
        assert!(unrelated_child.0.try_wait().unwrap().is_none());
        session.close().unwrap();
        until(|| control_directories(&f) == controls_before);
    }
}
#[test]
#[ignore = "requires native Unix PTY and actual exec failure; run with --ignored"]
fn native_exec_failure_does_not_release_input_or_abandon_owned_startup() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let invalid = f.path.join("invalid-exec");
    std::fs::write(&invalid, b"not an executable format\n").unwrap();
    std::fs::set_permissions(&invalid, std::fs::Permissions::from_mode(0o700)).unwrap();
    let launch = prepare(&f, &invalid, &[]);
    let result = NativeSession::spawn(
        launch,
        &f.path,
        &BTreeMap::new(),
        portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
    );
    let mut error = match result {
        Ok(mut session) => {
            session.close().unwrap();
            panic!("invalid command became ready");
        }
        Err(error) => error,
    };
    if let Some(session) = error.retained.as_mut() {
        assert!(session.write(b"must not run\n").is_err());
        until(|| session.close().is_ok());
    }
}
