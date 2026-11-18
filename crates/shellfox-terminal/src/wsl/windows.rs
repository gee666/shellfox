//! Own only the Windows wsl.exe transport. This Job grants no guest authority.
use super::WslError;
use std::{
    mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::Child,
    ptr,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
pub(super) struct TransportJob(OwnedHandle);
impl TransportJob {
    pub fn new() -> Result<Self, WslError> {
        // SAFETY: default non-inheritable Job, output structures have exact ABI sizes.
        let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err(WslError::Unavailable);
        }
        let job = Self(unsafe { OwnedHandle::from_raw_handle(raw.cast()) });
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle().cast(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                mem::size_of_val(&limits) as u32,
            )
        };
        if ok == 0 {
            return Err(WslError::Unavailable);
        }
        Ok(job)
    }
    pub fn assign(&self, child: &Child) -> Result<(), WslError> {
        // The trusted guest waits for its first frame before allocating a PTY.
        let process = child.as_raw_handle().cast();
        let assigned = unsafe { AssignProcessToJobObject(self.0.as_raw_handle().cast(), process) };
        let mut contained = 0;
        let checked =
            unsafe { IsProcessInJob(process, self.0.as_raw_handle().cast(), &mut contained) };
        if assigned == 0 || checked == 0 || contained == 0 {
            return Err(WslError::Unavailable);
        }
        Ok(())
    }
    pub fn finish(&self) -> Result<(), WslError> {
        let ok = unsafe { TerminateJobObject(self.0.as_raw_handle().cast(), 0) };
        if ok == 0 {
            return Err(WslError::Io);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { mem::zeroed() };
            let ok = unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle().cast(),
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    mem::size_of_val(&info) as u32,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(WslError::Io);
            }
            if info.ActiveProcesses == 0 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(WslError::Deadline);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
