use super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemOpenTarget {
    Directory(PathBuf),
    Web(String),
}
pub trait SystemOpener: Send + Sync {
    fn open(&self, target: SystemOpenTarget) -> SystemResult<()>;
}

/// Explicit host configuration, never an automatic boot side effect. Tests use
/// an injected recorder and never invoke this OS implementation.
pub struct NativeSystemOpener {
    pub interactive_session: bool,
}
impl SystemOpener for NativeSystemOpener {
    fn open(&self, target: SystemOpenTarget) -> SystemResult<()> {
        if !self.interactive_session {
            return Err(error(
                ErrorCode::Unsupported,
                "No authorized interactive opener is available on this host.",
            ));
        }
        match &target {
            SystemOpenTarget::Directory(path) => super::validation::absolute_path(path)?,
            SystemOpenTarget::Web(url) => {
                super::validation::web_url(url)?;
            }
        }
        #[cfg(windows)]
        {
            windows_open(target)
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix_open(target)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Err(error(
                ErrorCode::Unsupported,
                "System opening is unavailable on this platform.",
            ))
        }
    }
}
#[cfg(windows)]
fn windows_open(target: SystemOpenTarget) -> SystemResult<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            window: *mut c_void,
            verb: *const u16,
            file: *const u16,
            arguments: *const u16,
            directory: *const u16,
            show: i32,
        ) -> *mut c_void;
    }
    let value = match target {
        SystemOpenTarget::Directory(path) => path.into_os_string(),
        SystemOpenTarget::Web(url) => url.into(),
    };
    let value: Vec<u16> = value.encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    // SAFETY: validated directory/http(s) value, no arguments or shell command
    // interpolation. This dispatch occurs only for an explicit user action.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            value.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as isize <= 32 {
        return Err(error(
            ErrorCode::NativeUnavailable,
            "The registered system handler could not open the requested target.",
        ));
    }
    Ok(())
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn unix_open(target: SystemOpenTarget) -> SystemResult<()> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    #[cfg(target_os = "linux")]
    let program = "/usr/bin/xdg-open";
    #[cfg(target_os = "macos")]
    let program = "/usr/bin/open";
    if !std::path::Path::new(program).is_file() {
        return Err(error(
            ErrorCode::DependencyMissing,
            "The native desktop opener is not installed.",
        ));
    }
    let mut command = Command::new(program);
    #[cfg(target_os = "macos")]
    command.arg("--");
    match target {
        SystemOpenTarget::Directory(path) => {
            command.arg(path);
        }
        SystemOpenTarget::Web(url) => {
            command.arg(url);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if name == "BROWSER"
            || name == "BASH_ENV"
            || name == "ENV"
            || name.starts_with("LD_")
            || name.starts_with("DYLD_")
        {
            command.env_remove(key);
        }
    }
    command.env("PATH", "/usr/bin:/bin");
    let mut child = command.spawn().map_err(|_| {
        error(
            ErrorCode::NativeUnavailable,
            "System opener could not be started.",
        )
    })?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(error(
                        ErrorCode::NativeUnavailable,
                        "System opener rejected the target.",
                    ))
                }
            }
            Ok(None) => {}
            Err(_) => {
                return Err(error(
                    ErrorCode::NativeUnavailable,
                    "System opener completion could not be confirmed.",
                ))
            }
        }
        if Instant::now() >= deadline {
            // Never kill a browser/file-manager process to enforce a launcher
            // timeout. A reaper owns only this exact launcher child.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Err(error(
                ErrorCode::NativeUnavailable,
                "System opener did not confirm dispatch before its deadline.",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
