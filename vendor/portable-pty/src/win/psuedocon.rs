use super::WinChild;
use crate::cmdbuilder::CommandBuilder;
use crate::win::procthreadattr::ProcThreadAttributeList;
use anyhow::{bail, ensure, Error};
use filedescriptor::{FileDescriptor, OwnedHandle};
use lazy_static::lazy_static;
use shared_library::dynamic_library::DynamicLibrary;
use std::ffi::OsString;
use std::io::Error as IoError;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::sync::Mutex;
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::{HRESULT, S_OK};
use winapi::um::handleapi::*;
use winapi::um::processthreadsapi::*;
use winapi::um::winbase::{
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use winapi::um::wincon::COORD;
use winapi::um::winnt::HANDLE;

pub type HPCON = HANDLE;

const PSEUDOCONSOLE_RESIZE_QUIRK: DWORD = 0x2;
const PSEUDOCONSOLE_WIN32_INPUT_MODE: DWORD = 0x4;

type CreateFn = unsafe extern "system" fn(COORD, HANDLE, HANDLE, DWORD, *mut HPCON) -> HRESULT;
type ResizeFn = unsafe extern "system" fn(HPCON, COORD) -> HRESULT;
type CloseFn = unsafe extern "system" fn(HPCON);

struct ConPtyFuncs {
    create: CreateFn,
    resize: ResizeFn,
    close: CloseFn,
    flags: DWORD,
}

impl ConPtyFuncs {
    /// Resolves `<prefix>CreatePseudoConsole` and friends. The library stays loaded.
    fn open(path: &Path, prefix: &str, flags: DWORD) -> Result<Self, String> {
        let lib = DynamicLibrary::open(Some(path))?;
        let symbol = |name: &str| unsafe { lib.symbol::<u8>(&format!("{prefix}{name}")) };
        let (create, resize, close) = (
            symbol("CreatePseudoConsole")?,
            symbol("ResizePseudoConsole")?,
            symbol("ClosePseudoConsole")?,
        );
        mem::forget(lib);
        Ok(unsafe {
            Self {
                create: mem::transmute::<*mut u8, CreateFn>(create),
                resize: mem::transmute::<*mut u8, ResizeFn>(resize),
                close: mem::transmute::<*mut u8, CloseFn>(close),
                flags,
            }
        })
    }
}

/// Prefers Microsoft's conpty.dll beside the executable, which starts the
/// OpenConsole.exe next to it. Unlike the inbox kernel32 ConPTY it passes
/// mouse modes and most VT output through, so full-screen TUIs get wheel
/// events. The inbox ConPTY is the fallback when the files are missing.
fn load_conpty() -> ConPtyFuncs {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("conpty.dll")))
        .filter(|dll| dll.is_file());
    if let Some(dll) = bundled {
        // The bundled ConPTY always negotiates win32-input-mode; it reads no other flags.
        match ConPtyFuncs::open(&dll, "Conpty", 0) {
            Ok(funcs) => return funcs,
            Err(err) => log::warn!("{} is unusable, using the inbox ConPTY: {}", dll.display(), err),
        }
    }
    ConPtyFuncs::open(
        Path::new("kernel32.dll"),
        "",
        PSEUDOCONSOLE_RESIZE_QUIRK | PSEUDOCONSOLE_WIN32_INPUT_MODE,
    )
    .expect("this system does not support conpty.  Windows 10 October 2018 or newer is required")
}

lazy_static! {
    static ref CONPTY: ConPtyFuncs = load_conpty();
}

pub struct PsuedoCon {
    con: HPCON,
}

unsafe impl Send for PsuedoCon {}
unsafe impl Sync for PsuedoCon {}

impl Drop for PsuedoCon {
    fn drop(&mut self) {
        unsafe { (CONPTY.close)(self.con) };
    }
}

impl PsuedoCon {
    pub fn new(size: COORD, input: FileDescriptor, output: FileDescriptor) -> Result<Self, Error> {
        let mut con: HPCON = INVALID_HANDLE_VALUE;
        let result = unsafe {
            (CONPTY.create)(
                size,
                input.as_raw_handle() as _,
                output.as_raw_handle() as _,
                CONPTY.flags,
                &mut con,
            )
        };
        ensure!(
            result == S_OK,
            "failed to create psuedo console: HRESULT {}",
            result
        );
        Ok(Self { con })
    }

    pub fn resize(&self, size: COORD) -> Result<(), Error> {
        let result = unsafe { (CONPTY.resize)(self.con, size) };
        ensure!(
            result == S_OK,
            "failed to resize console to {}x{}: HRESULT: {}",
            size.X,
            size.Y,
            result
        );
        Ok(())
    }

    pub fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<WinChild> {
        let mut si: STARTUPINFOEXW = unsafe { mem::zeroed() };
        si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
        // Explicitly set the stdio handles as invalid handles otherwise
        // we can end up with a weird state where the spawned process can
        // inherit the explicitly redirected output handles from its parent.
        // For example, when daemonizing wezterm-mux-server, the stdio handles
        // are redirected to a log file and the spawned process would end up
        // writing its output there instead of to the pty we just created.
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;

        let mut attrs = ProcThreadAttributeList::with_capacity(1)?;
        attrs.set_pty(self.con)?;
        si.lpAttributeList = attrs.as_mut_ptr();

        let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };

        let (mut exe, mut cmdline) = cmd.cmdline()?;
        let cmd_os = OsString::from_wide(&cmdline);

        let cwd = cmd.current_directory();

        let res = unsafe {
            CreateProcessW(
                exe.as_mut_slice().as_mut_ptr(),
                cmdline.as_mut_slice().as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                cmd.environment_block().as_mut_slice().as_mut_ptr() as *mut _,
                cwd.as_ref()
                    .map(|c| c.as_slice().as_ptr())
                    .unwrap_or(ptr::null()),
                &mut si.StartupInfo,
                &mut pi,
            )
        };
        if res == 0 {
            let err = IoError::last_os_error();
            let msg = format!(
                "CreateProcessW `{:?}` in cwd `{:?}` failed: {}",
                cmd_os,
                cwd.as_ref().map(|c| OsString::from_wide(c)),
                err
            );
            log::error!("{}", msg);
            bail!("{}", msg);
        }

        // Make sure we close out the thread handle so we don't leak it;
        // we do this simply by making it owned
        let _main_thread = unsafe { OwnedHandle::from_raw_handle(pi.hThread as _) };
        let proc = unsafe { OwnedHandle::from_raw_handle(pi.hProcess as _) };

        Ok(WinChild {
            proc: Mutex::new(proc),
        })
    }
}
