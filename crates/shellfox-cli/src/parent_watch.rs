//! A forwarded CLI stops with the launcher that started it, as 0.9.x did.
//! The 0.9.x Node launcher passes an IPC channel (`NODE_CHANNEL_FD`); the
//! Windows Rust launcher passes its PID. Neither leaks into host terminals.

/// Set by the Windows Rust launcher to its own PID.
pub const FORWARDER: &str = "SHELLFOX_HEADLESS_FORWARDER_PID";
const NODE_CHANNEL: &str = "NODE_CHANNEL_FD";

/// Claims the inherited launcher link and clears it from this environment.
/// Call once at startup, before any threads or child processes exist.
pub fn claim() -> Option<Watch> {
    let node = std::env::var(NODE_CHANNEL).ok();
    let forwarder = std::env::var(FORWARDER).ok();
    for key in [NODE_CHANNEL, "NODE_CHANNEL_SERIALIZATION_MODE", FORWARDER] {
        std::env::remove_var(key);
    }
    platform::claim(node, forwarder)
}

pub struct Watch(platform::Link);
impl Watch {
    /// Runs `on_exit` on a background thread once the launcher is gone.
    pub fn spawn(self, on_exit: impl FnOnce() + Send + 'static) {
        let link = self.0;
        std::thread::Builder::new()
            .name("launcher-watch".into())
            .spawn(move || {
                if platform::wait(link) {
                    on_exit();
                }
            })
            .ok();
    }
}

/// The launcher forwards Ctrl+C semantics to its child; it must not die first.
/// A registered handler, unlike the NULL "ignore" flag, is not inherited.
#[cfg(windows)]
pub fn ignore_console_interrupts() {
    use windows_sys::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT,
    };
    unsafe extern "system" fn swallow(event: u32) -> i32 {
        i32::from(event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT)
    }
    // SAFETY: registers a static handler for this process only.
    unsafe {
        SetConsoleCtrlHandler(Some(swallow), 1);
    }
}

#[cfg(unix)]
mod platform {
    use std::{
        fs::File,
        io::Read,
        os::{
            fd::{FromRawFd, OwnedFd},
            unix::fs::FileTypeExt,
        },
    };
    pub type Link = File;
    pub fn claim(node: Option<String>, _forwarder: Option<String>) -> Option<super::Watch> {
        let fd: i32 = node?.parse().ok().filter(|fd| *fd > 2)?;
        // A stale variable must not take over an unrelated descriptor.
        // SAFETY: the borrowed File is never dropped, so the fd is not closed.
        let probe = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(fd) });
        if !probe
            .metadata()
            .is_ok_and(|info| info.file_type().is_socket())
        {
            return None;
        }
        // SAFETY: Node passes this socket to us and nothing else in this
        // process has opened or wrapped it yet.
        let inherited = unsafe { OwnedFd::from_raw_fd(fd) };
        // The duplicate is close-on-exec; dropping the original keeps the
        // channel out of terminals and helpers started by the host.
        let private = inherited.try_clone().ok()?;
        drop(inherited);
        Some(super::Watch(File::from(private)))
    }
    pub fn wait(mut link: File) -> bool {
        let mut buffer = [0; 4096];
        loop {
            match link.read(&mut buffer) {
                Ok(0) => return true,
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return true,
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
            WAIT_OBJECT_0,
        },
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE},
        },
    };
    pub struct Link(HANDLE);
    // SAFETY: a process handle may be waited on and closed from any thread.
    unsafe impl Send for Link {}
    extern "C" {
        fn _get_osfhandle(fd: i32) -> isize;
    }
    pub fn claim(node: Option<String>, forwarder: Option<String>) -> Option<super::Watch> {
        // Node's IPC pipe is CRT fd N and inheritable. Keep it out of the host's
        // children, or the 0.9 launcher waits for them after the host exits.
        if let Some(fd) = node
            .as_deref()
            .and_then(|fd| fd.parse::<i32>().ok())
            .filter(|fd| *fd > 2)
        {
            // SAFETY: _get_osfhandle validates the fd; the handle stays owned by the CRT.
            let handle = unsafe { _get_osfhandle(fd) };
            if handle != -1 && handle != -2 {
                // SAFETY: only clears the inherit flag of a handle this process owns.
                unsafe { SetHandleInformation(handle as HANDLE, HANDLE_FLAG_INHERIT, 0) };
            }
        }
        let pid = match forwarder {
            Some(pid) => pid.parse().ok()?,
            None => {
                node?;
                parent_pid()?
            }
        };
        // SAFETY: plain Win32 call; a null result is handled.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        (!handle.is_null()).then_some(super::Watch(Link(handle)))
    }
    pub fn wait(link: Link) -> bool {
        // SAFETY: the handle is owned by `link` and closed exactly once.
        unsafe {
            let done = WaitForSingleObject(link.0, INFINITE) == WAIT_OBJECT_0;
            CloseHandle(link.0);
            done
        }
    }
    fn parent_pid() -> Option<u32> {
        let own = std::process::id();
        // SAFETY: the snapshot handle is closed before returning and the
        // entry struct is initialized with its size as the API requires.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if entry.th32ProcessID == own {
                    found = Some(entry.th32ParentProcessID);
                    break;
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            found
        }
    }
}
