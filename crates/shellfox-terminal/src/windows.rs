use anyhow::{bail, ensure, Context, Result};
use portable_pty::{Child, ExitStatus};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::{io, mem, ptr};
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::*;

pub(crate) const SUPERVISOR_FLAG: &str = "--shellfox-pty-supervisor";
pub(crate) const STARTUP_TIMEOUT_MS: u32 = 5000;

pub(crate) fn system_directory() -> Result<std::path::PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let len = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    ensure!(
        len > 0 && len < buffer.len(),
        "get trusted system directory: {}",
        io::Error::last_os_error()
    );
    Ok(std::path::PathBuf::from(OsString::from_wide(
        &buffer[..len],
    )))
}

/// Query an owned metadata-only, nofollow file handle. No contents are read.
pub(crate) fn validate_single_link_file(path: &std::path::Path) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("open isolated file metadata {}", path.display()))?;
    let mut info = mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: file retains a valid handle; the output buffer is writable and is
    // read only after Windows reports a successful complete structure write.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), info.as_mut_ptr()) };
    ensure!(
        ok != 0,
        "query isolated file metadata: {}",
        io::Error::last_os_error()
    );
    // SAFETY: GetFileInformationByHandle initialized info on success above.
    let info = unsafe { info.assume_init() };
    ensure!(
        info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) == 0,
        "isolated file handle is not a regular non-reparse file: {}",
        path.display()
    );
    ensure!(
        info.nNumberOfLinks == 1,
        "isolated file must not have hardlink aliases: {}",
        path.display()
    );
    Ok(())
}

pub(crate) fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn owned(handle: HANDLE) -> Result<OwnedHandle> {
    ensure!(
        !handle.is_null(),
        "Windows handle creation failed: {}",
        io::Error::last_os_error()
    );
    // SAFETY: caller passes a new, non-null handle whose sole ownership transfers here.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.cast()) })
}

pub(crate) struct Job(OwnedHandle);

// Job completion-port messages (winnt.h).
const JOB_MESSAGE_ACTIVE_PROCESS_ZERO: u32 = 4;
const JOB_MESSAGE_EXIT_PROCESS: u32 = 7;
const JOB_MESSAGE_ABNORMAL_EXIT_PROCESS: u32 = 8;

impl Job {
    pub fn new() -> Result<Self> {
        // Null security attributes make this handle noninheritable.
        let job = Self(owned(unsafe {
            CreateJobObjectW(ptr::null(), ptr::null())
        })?);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                job.handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                mem::size_of_val(&limits) as u32,
            )
        };
        ensure!(ok != 0, "set Job limits: {}", io::Error::last_os_error());
        Ok(job)
    }

    pub(crate) fn handle(&self) -> HANDLE {
        self.0.as_raw_handle().cast()
    }

    /// Calls `wake` after each process in the Job exits and once the Job is
    /// empty. A Job takes one completion port, so call this once.
    pub fn watch(&self, wake: crate::Wake) -> Result<()> {
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus},
        };
        let port =
            owned(unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1) })?;
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: ptr::null_mut(),
            CompletionPort: port.as_raw_handle().cast(),
        };
        let ok = unsafe {
            SetInformationJobObject(
                self.handle(),
                JobObjectAssociateCompletionPortInformation,
                (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                mem::size_of_val(&association) as u32,
            )
        };
        ensure!(ok != 0, "watch owned Job: {}", io::Error::last_os_error());
        // Windows posts ACTIVE_PROCESS_ZERO only on a transition, so an
        // already empty Job gets no message.
        if self.active_processes()? == 0 {
            wake();
            return Ok(());
        }
        std::thread::Builder::new()
            .name("shellfox-job-watch".into())
            .spawn(move || loop {
                let (mut message, mut key, mut overlapped) = (0, 0, ptr::null_mut());
                // SAFETY: `port` stays open for the whole wait; the outputs are locals.
                let ok = unsafe {
                    GetQueuedCompletionStatus(
                        port.as_raw_handle().cast(),
                        &mut message,
                        &mut key,
                        &mut overlapped,
                        INFINITE,
                    )
                };
                if ok == 0 {
                    break;
                }
                match message {
                    JOB_MESSAGE_EXIT_PROCESS | JOB_MESSAGE_ABNORMAL_EXIT_PROCESS => wake(),
                    JOB_MESSAGE_ACTIVE_PROCESS_ZERO => {
                        wake();
                        break;
                    }
                    _ => {}
                }
            })
            .context("start Job watch")?;
        Ok(())
    }

    pub fn assign(&self, child: &(dyn Child + Send + Sync)) -> Result<()> {
        let process = child_handle(child)?;
        let ok = unsafe { AssignProcessToJobObject(self.handle(), process) };
        ensure!(
            ok != 0,
            "assign supervisor to Job: {}",
            io::Error::last_os_error()
        );
        let mut contained = 0;
        let ok = unsafe { IsProcessInJob(process, self.handle(), &mut contained) };
        ensure!(
            ok != 0 && contained != 0,
            "supervisor Job containment not confirmed"
        );
        Ok(())
    }

    pub fn terminate(&self) -> Result<()> {
        let ok = unsafe { TerminateJobObject(self.handle(), 1) };
        ensure!(
            ok != 0,
            "terminate owned Job: {}",
            io::Error::last_os_error()
        );
        Ok(())
    }

    pub fn active_processes(&self) -> Result<u32> {
        let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                mem::size_of_val(&accounting) as u32,
                ptr::null_mut(),
            )
        };
        ensure!(ok != 0, "query owned Job: {}", io::Error::last_os_error());
        Ok(accounting.ActiveProcesses)
    }
}

pub(crate) struct StartupGate {
    release: OwnedHandle,
    ready: OwnedHandle,
    pub release_name: String,
    pub ready_name: String,
}

impl StartupGate {
    pub fn new() -> Result<Self> {
        let mut random = [0u8; 16];
        let status = unsafe {
            BCryptGenRandom(
                ptr::null_mut(),
                random.as_mut_ptr(),
                random.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        ensure!(
            status >= 0,
            "generate supervisor startup nonce: {status:#x}"
        );
        let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let release_name = format!("Local\\ShellfoxPty-{nonce}-GO");
        let ready_name = format!("Local\\ShellfoxPty-{nonce}-READY");
        // Default DACL comes from the current token. Handles are noninheritable.
        let release = create_event(&release_name)?;
        let ready = create_event(&ready_name)?;
        Ok(Self {
            release,
            ready,
            release_name,
            ready_name,
        })
    }

    pub fn release(&self) -> Result<()> {
        let ok = unsafe { SetEvent(self.release.as_raw_handle().cast()) };
        ensure!(
            ok != 0,
            "release supervisor: {}",
            io::Error::last_os_error()
        );
        Ok(())
    }

    pub fn ready(&self) -> Result<bool> {
        wait(self.ready.as_raw_handle().cast(), 0)
    }

    /// Waits for READY until `process` exits or `deadline` passes. True means READY.
    pub fn wait_ready(&self, process: HANDLE, deadline: std::time::Instant) -> Result<bool> {
        let handles = [self.ready.as_raw_handle().cast(), process];
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        // INFINITE is u32::MAX, so cap one below it.
        let millis = u32::try_from(left.as_millis())
            .unwrap_or(u32::MAX - 1)
            .min(u32::MAX - 1);
        match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, millis) } {
            WAIT_OBJECT_0 => Ok(true),
            result if result == WAIT_OBJECT_0 + 1 || result == WAIT_TIMEOUT => Ok(false),
            _ => bail!("wait for supervisor READY: {}", io::Error::last_os_error()),
        }
    }
}

fn create_event(name: &str) -> Result<OwnedHandle> {
    let handle = unsafe { CreateEventW(ptr::null(), 1, 0, wide(name).as_ptr()) };
    let error = unsafe { GetLastError() };
    let event = owned(handle)?;
    ensure!(
        error != ERROR_ALREADY_EXISTS,
        "startup event name already exists"
    );
    Ok(event)
}

pub(crate) fn open_event(name: &str, access: u32) -> Result<OwnedHandle> {
    owned(unsafe { OpenEventW(access, 0, wide(name).as_ptr()) })
        .context("open supervisor startup event")
}

pub(crate) fn wait(handle: HANDLE, millis: u32) -> Result<bool> {
    match unsafe { WaitForSingleObject(handle, millis) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => bail!("wait for owned handle: {}", io::Error::last_os_error()),
    }
}

fn child_handle(child: &(dyn Child + Send + Sync)) -> Result<HANDLE> {
    child
        .as_raw_handle()
        .map(|handle| handle.cast())
        .context("supervisor has no owned process handle")
}

pub(crate) fn poll_child(child: &(dyn Child + Send + Sync)) -> Result<Option<ExitStatus>> {
    let process = child_handle(child)?;
    if !wait(process, 0)? {
        return Ok(None);
    }
    let mut code = 0;
    let ok = unsafe { GetExitCodeProcess(process, &mut code) };
    ensure!(
        ok != 0,
        "read supervisor exit status: {}",
        io::Error::last_os_error()
    );
    Ok(Some(ExitStatus::with_exit_code(code)))
}

/// Only used before release, when the trusted supervisor cannot have spawned children.
pub(crate) fn terminate_unreleased(child: &(dyn Child + Send + Sync)) -> Result<()> {
    if poll_child(child)?.is_some() {
        return Ok(());
    }
    let ok = unsafe { TerminateProcess(child_handle(child)?, 1) };
    if ok == 0 && poll_child(child)?.is_none() {
        bail!(
            "terminate unreleased supervisor: {}",
            io::Error::last_os_error()
        );
    }
    Ok(())
}
